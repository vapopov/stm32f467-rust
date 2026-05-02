
#![no_std]
#![no_main]

use defmt::*;
use defmt::{info, unwrap};
use {defmt_rtt as _, panic_probe as _};
use static_cell::StaticCell;

use embassy_futures::yield_now;
use embassy_executor::Spawner;
use embassy_net::{Stack, Ipv4Address, StackResources, Ipv4Cidr};
use embassy_net::icmp::{ChecksumCapabilities, IcmpEndpoint, IcmpSocket, Icmpv4Packet, Icmpv4Repr, PacketMetadata};

use embassy_stm32::eth::{Ethernet, PacketQueue, GenericPhy,Sma};
use embassy_stm32::peripherals::{ETH, ETH_SMA};
use embassy_stm32::rng::Rng;

use embassy_stm32::time::Hertz;
use embassy_stm32::{bind_interrupts, eth, peripherals, rng, Config};

use embassy_time::{Timer, Delay, Instant};

use embassy_stm32::fmc::Fmc;
use heapless::Vec;

use stm32_fmc;

bind_interrupts!(struct Irqs {
    ETH => eth::InterruptHandler;
    RNG => rng::InterruptHandler<peripherals::RNG>;
});

type Device = Ethernet<'static, ETH, GenericPhy<Sma<'static, ETH_SMA>>>;

#[embassy_executor::task]
async fn net_task(mut runner: embassy_net::Runner<'static, Device>) -> ! {
    runner.run().await
}

#[embassy_executor::main]
async fn main(spawner: Spawner) -> ! {
    let mut config = Config::default();
    {
        use embassy_stm32::rcc::*;
        config.rcc.hse = Some(Hse {
            freq: Hertz(25_000_000),
            mode: HseMode::Oscillator,
        });
        config.rcc.pll_src = PllSource::HSE;
        // 25mhz / 25 * 432 / 2 = 216Mhz
        config.rcc.pll = Some(Pll {
            prediv: PllPreDiv::DIV25,
            mul: PllMul::MUL432,
            divp: Some(PllPDiv::DIV2),
            divq: Some(PllQDiv::DIV2),
            divr: None,
        });
        config.rcc.ahb_pre = AHBPrescaler::DIV1;
        config.rcc.apb1_pre = APBPrescaler::DIV4;
        config.rcc.apb2_pre = APBPrescaler::DIV2;
        config.rcc.sys = Sysclk::PLL1_P;
        // RCC_OscInitStruct.OscillatorType = RCC_OSCILLATORTYPE_HSE;
        // RCC_OscInitStruct.HSEState = RCC_HSE_ON;
        // RCC_OscInitStruct.PLL.PLLState = RCC_PLL_ON;
        // RCC_OscInitStruct.PLL.PLLSource = RCC_PLLSOURCE_HSE;
        // RCC_OscInitStruct.PLL.PLLM = 25;
        // RCC_OscInitStruct.PLL.PLLN = 432;
        // RCC_OscInitStruct.PLL.PLLP = RCC_PLLP_DIV2;
        // RCC_OscInitStruct.PLL.PLLQ = 2;
    }
    let p = embassy_stm32::init(config);

    info!("Hello World!");

    let mut core_peri = cortex_m::Peripherals::take().unwrap();

    // taken from stm32h7xx-hal
    core_peri.SCB.enable_icache();
    // See Errata Sheet 2.2.1
    // core_peri.SCB.enable_dcache(&mut core_peri.CPUID);
    core_peri.DWT.enable_cycle_counter();
    // -----------------------------------
    // ----------------------------------------------------------
    // Configure MPU for external SDRAM
    // MPU config for SDRAM write-through
    let sdram_size = 8 * 1024 * 1024;

    {
        let mpu = core_peri.MPU;
        let scb = &mut core_peri.SCB;
        let size = sdram_size;
        // Refer to ARM®v7-M Architecture Reference Manual ARM DDI 0403
        // Version E.b Section B3.5
        const MEMFAULTENA: u32 = 1 << 16;

        unsafe {
            /* Make sure outstanding transfers are done */
            cortex_m::asm::dmb();

            scb.shcsr.modify(|r| r & !MEMFAULTENA);

            /* Disable the MPU and clear the control register*/
            mpu.ctrl.write(0);
        }

        const REGION_NUMBER0: u32 = 0x00;
        const REGION_BASE_ADDRESS: u32 = 0xC000_0000;
        const REGION_FULL_ACCESS: u32 = 0x03;
        const REGION_CACHEABLE: u32 = 0x01;
        const REGION_WRITE_BACK: u32 = 0x01;
        const REGION_ENABLE: u32 = 0x01;

        crate::assert_eq!(size & (size - 1), 0, "SDRAM memory region size must be a power of 2");
        crate::assert_eq!(size & 0x1F, 0, "SDRAM memory region size must be 32 bytes or more");
        fn log2minus1(sz: u32) -> u32 {
            for i in 5..=31 {
                if sz == (1 << i) {
                    return i - 1;
                }
            }
            crate::panic!("Unknown SDRAM memory region size!");
            sz
        }

        //info!("SDRAM Memory Size 0x{:x}", log2minus1(size as u32));

        // Configure region 0
        //
        // Cacheable, outer and inner write-back, no write allocate. So
        // reads are cached, but writes always write all the way to SDRAM
        unsafe {
            mpu.rnr.write(REGION_NUMBER0);
            mpu.rbar.write(REGION_BASE_ADDRESS);
            mpu.rasr.write(
                (REGION_FULL_ACCESS << 24)
                    | (REGION_CACHEABLE << 17)
                    | (REGION_WRITE_BACK << 16)
                    | (log2minus1(size as u32) << 1)
                    | REGION_ENABLE,
            );
        }

        const MPU_ENABLE: u32 = 0x01;
        const MPU_DEFAULT_MMAP_FOR_PRIVILEGED: u32 = 0x04;

        // Enable
        unsafe {
            mpu.ctrl.modify(|r| r | MPU_DEFAULT_MMAP_FOR_PRIVILEGED | MPU_ENABLE);

            scb.shcsr.modify(|r| r | MEMFAULTENA);

            // Ensure MPU settings take effect
            cortex_m::asm::dsb();
            cortex_m::asm::isb();
        }
    }

    let mut sdram = Fmc::sdram_a12bits_d16bits_4banks_bank1(
        p.FMC,
        // A0-A11
        p.PF0,
        p.PF1,
        p.PF2,
        p.PF3,
        p.PF4,
        p.PF5,
        p.PF12,
        p.PF13,
        p.PF14,
        p.PF15,
        p.PG0,
        p.PG1,
        // BA0-BA1
        p.PG4,
        p.PG5,
        // D0-D31
        p.PD14,
        p.PD15,
        p.PD0,
        p.PD1,
        p.PE7,
        p.PE8,
        p.PE9,
        p.PE10,
        p.PE11,
        p.PE12,
        p.PE13,
        p.PE14,
        p.PE15,
        p.PD8,
        p.PD9,
        p.PD10,

        // NBL0 - NBL3
        p.PE0,
        p.PE1,
        p.PC3,  // SDCKE0
        p.PG8,  // SDCLK
        p.PG15, // SDNCAS
        p.PH3,  // SDNE0 (!CS)
        p.PF11, // SDRAS
        p.PH5,  // SDNWE, change to p.PH5 for EVAL boards
        stm32_fmc::devices::is42s32400f_6::Is42s32400f6 {
        },
    );

    let mut delay = Delay;

    let ram_slice = unsafe {
        // Initialise controller and SDRAM
        let ram_ptr: *mut u32 = sdram.init(&mut delay) as *mut _;

        // Convert raw pointer to slice
        core::slice::from_raw_parts_mut(ram_ptr, sdram_size / core::mem::size_of::<u32>())
    };

    // Generate random seed.
    let mut rng = Rng::new(p.RNG, Irqs);
    let mut seed = [0; 8];
    rng.fill_bytes(&mut seed);
    let seed = u64::from_le_bytes(seed);

    // let mac_addr = [0x00, 0x00, 0xDE, 0xAD, 0xBE, 0xEF];
    let mac_addr = [0x2E, 0x8F, 0x21, 0x6C, 0xBE, 0x5A];
    static PACKETS: StaticCell<PacketQueue<4, 4>> = StaticCell::new();

    let device = Ethernet::new(
        PACKETS.init(PacketQueue::<4, 4>::new()),
        p.ETH,
        Irqs,
        p.PA1,
        p.PA7,
        p.PC4,
        p.PC5,
        p.PG13,
        p.PG14,
        p.PG11,
        mac_addr,
        p.ETH_SMA,
        p.PA2, // mdio
        p.PC1, // mdc
    );

    let config = embassy_net::Config::dhcpv4(Default::default());
    // let config = embassy_net::Config::ipv4_static(embassy_net::StaticConfigV4 {
    //    address: Ipv4Cidr::new(Ipv4Address::new(192, 168, 8, 233), 24),
    //    dns_servers: Vec::new(),
    //    gateway: Some(Ipv4Address::new(192, 168, 8, 1)),
    // });

    // Init network stack
    static RESOURCES: StaticCell<StackResources<3>> = StaticCell::new();
    let (stack, runner) = embassy_net::new(device, config, RESOURCES.init(StackResources::new()), seed);

    // Launch network task
    spawner.spawn(unwrap!(net_task(runner)));

    // Ensure DHCP configuration is up before trying connect
    // stack.wait_config_up().await;
    info!("Waiting for DHCP...");
    let cfg = wait_for_config(stack).await;
    let local_addr = cfg.address.address();
    info!("IP address: {:?}", local_addr);

    // Then we can use it!
    let mut rx_buffer = [0; 256];
    let mut tx_buffer = [0; 256];
    let mut rx_meta = [PacketMetadata::EMPTY];
    let mut tx_meta = [PacketMetadata::EMPTY];

    // Identifier used for the ICMP socket
    let ident = 42;

    // Create and bind the socket
    let mut socket = IcmpSocket::new(stack, &mut rx_meta, &mut rx_buffer, &mut tx_meta, &mut tx_buffer);
    socket.bind(IcmpEndpoint::Ident(ident)).unwrap();

    // Create the repr for the packet
    let icmp_repr = Icmpv4Repr::EchoRequest {
        ident,
        seq_no: 0,
        data: b"Hello, icmp!",
    };

    // Send the packet and store the starting instant to mesure latency later
    let start = socket
        .send_to_with(icmp_repr.buffer_len(), cfg.gateway.unwrap(), |buf| {
            // Create and populate the packet buffer allocated by `send_to_with`
            let mut icmp_packet = Icmpv4Packet::new_unchecked(buf);
            icmp_repr.emit(&mut icmp_packet, &ChecksumCapabilities::default());
            (icmp_repr.buffer_len(), Instant::now()) // Return the instant where the packet was sent
        })
        .await
        .unwrap();

    // Recieve and log the data of the reply
    socket
        .recv_from_with(|(buf, addr)| {
            let packet = Icmpv4Packet::new_checked(buf).unwrap();
            info!(
                "Recieved {:?} from {} in {}ms",
                packet.data(),
                addr,
                start.elapsed().as_millis()
            );
        })
        .await
        .unwrap();

    // // ----------------------------------------------------------
    // // Use memory in SDRAM
    info!("RAM contents before writing: {:x}", ram_slice[..10]);

    ram_slice[0] = 1;
    ram_slice[1] = 2;
    ram_slice[2] = 3;
    ram_slice[3] = 4;
    ram_slice[4] = 5;
    ram_slice[5] = 6;
    ram_slice[6] = 7;
    ram_slice[7] = 8;
    ram_slice[8] = 0xde;
    ram_slice[9] = 0xad;

    info!("RAM contents after writing: {:x}", ram_slice[..10]);

    crate::assert_eq!(ram_slice[0], 1);
    crate::assert_eq!(ram_slice[1], 2);
    crate::assert_eq!(ram_slice[2], 3);
    crate::assert_eq!(ram_slice[3], 4);

    info!("Assertions succeeded.");

    loop {
        Timer::after_secs(1).await;
        info!("RAM contents after writing: {:x}", ram_slice[..10]);
    }
}

async fn wait_for_config(stack: Stack<'static>) -> embassy_net::StaticConfigV4 {
    loop {
        if let Some(config) = stack.config_v4() {
            return config.clone();
        }
        yield_now().await;
    }
}