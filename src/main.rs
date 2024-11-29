#![no_std]
#![no_main]

use defmt::{info, unwrap};
use embassy_executor::Spawner;
use embassy_net::{Ipv4Address, StackResources, Ipv4Cidr};
use embassy_stm32::eth::generic_smi::GenericSMI;
use embassy_stm32::eth::{Ethernet, PacketQueue};
use embassy_stm32::peripherals::ETH;
use embassy_stm32::rng::Rng;
use embassy_stm32::time::Hertz;
use embassy_stm32::{bind_interrupts, eth, peripherals, rng, Config};
use embassy_time::Timer;
use rand_core::RngCore;
use static_cell::StaticCell;
use heapless::Vec;

use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    ETH => eth::InterruptHandler;
    RNG => rng::InterruptHandler<peripherals::RNG>;
});

type Device = Ethernet<'static, ETH, GenericSMI>;

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

    // Generate random seed.
    let mut rng = Rng::new(p.RNG, Irqs);
    let mut seed = [0; 8];
    rng.fill_bytes(&mut seed);
    let seed = u64::from_le_bytes(seed);

    let mac_addr = [0x00, 0x00, 0xDE, 0xAD, 0xBE, 0xEF];
    static PACKETS: StaticCell<PacketQueue<4, 4>> = StaticCell::new();

    let device = Ethernet::new(
        PACKETS.init(PacketQueue::<4, 4>::new()),
        p.ETH,
        Irqs,
        p.PA1,
        p.PA2,
        p.PC1,
        p.PA7,
        p.PC4,
        p.PC5,
        p.PG13,
        p.PG14,
        p.PG11,
        GenericSMI::new(0),
        mac_addr,
    );

    let config = embassy_net::Config::ipv4_static(embassy_net::StaticConfigV4 {
       address: Ipv4Cidr::new(Ipv4Address::new(192, 168, 0, 161), 24),
       dns_servers: Vec::new(),
       gateway: None, // Some(Ipv4Address::new(10, 42, 0, 1)),
    });

    // Init network stack
    static RESOURCES: StaticCell<StackResources<3>> = StaticCell::new();
    let (stack, runner) = embassy_net::new(device, config, RESOURCES.init(StackResources::new()), seed);

    // Launch network task
    unwrap!(spawner.spawn(net_task(runner)));

    // Ensure DHCP configuration is up before trying to connect when dhcp is used.
    // stack.wait_config_up().await;

    info!("Network task initialized");

    loop {
        info!("Hello World!");
        Timer::after_secs(1).await;
    }
}
