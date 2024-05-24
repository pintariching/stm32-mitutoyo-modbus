use defmt_rtt as _;
use panic_probe as _;

use stm32_eth::hal::rcc::Clocks;
use stm32_eth::stm32::Peripherals;
use stm32_eth::PartsIn;
pub use stm32f4xx_hal::gpio::*;
use stm32f4xx_hal::prelude::*;
use stm32f4xx_hal::rcc::RccExt;

pub struct Gpio {
    pub gpioa: gpioa::Parts,
    pub gpiob: gpiob::Parts,
    pub gpioc: gpioc::Parts,
    pub gpiod: gpiod::Parts,
    pub gpioe: gpioe::Parts,
    pub gpiof: gpiof::Parts,
    pub gpiog: gpiog::Parts,
}

pub fn setup_peripherals(p: Peripherals) -> (Clocks, Gpio, PartsIn) {
    let ethernet = PartsIn {
        dma: p.ETHERNET_DMA,
        mac: p.ETHERNET_MAC,
        mmc: p.ETHERNET_MMC,
        ptp: p.ETHERNET_PTP,
    };

    let rcc = p.RCC.constrain();

    let clocks = rcc.cfgr.sysclk(96.MHz()).hclk(96.MHz());

    let clocks = {
        if cfg!(hse = "bypass") {
            clocks.use_hse(8.MHz()).bypass_hse_oscillator()
        } else if cfg!(hse = "oscillator") {
            clocks.use_hse(8.MHz())
        } else {
            clocks
        }
    };

    let clocks = clocks.freeze();

    let gpio = Gpio {
        gpioa: p.GPIOA.split(),
        gpiob: p.GPIOB.split(),
        gpioc: p.GPIOC.split(),
        gpiod: p.GPIOD.split(),
        gpioe: p.GPIOE.split(),
        gpiof: p.GPIOF.split(),
        gpiog: p.GPIOG.split(),
    };

    (clocks, gpio, ethernet)
}
