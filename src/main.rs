#![no_std]
#![no_main]

extern crate cortex_m;
extern crate stm32f7xx_hal;

use cortex_m_rt::entry;
use core::panic::PanicInfo;
use rtt_target::{rprintln, rtt_init_print};

#[entry]
fn main() -> ! {
    rtt_init_print!();
    rprintln!("Hello, world!");
    loop {
        rprintln!("Hello, world!+");
        for _ in 0..1_000_000 {
            cortex_m::asm::nop();
        }
    }
}

#[inline(never)]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    rprintln!("{}", info);
    loop {} // You might need a compiler fence in here.
}