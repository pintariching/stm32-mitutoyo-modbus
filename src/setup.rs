use defmt_rtt as _;
use panic_probe as _;

use stm32_eth::hal::rcc::Clocks;
use stm32_eth::stm32::Peripherals;
use stm32_eth::{EthPins, PartsIn};
pub use stm32f4xx_hal::gpio::*;
use stm32f4xx_hal::prelude::*;
use stm32f4xx_hal::rcc::RccExt;

pub struct Gpio {
    pub gpioa: gpioa::Parts,
    pub gpiob: gpiob::Parts,
    pub gpioc: gpioc::Parts,
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
        gpiog: p.GPIOG.split(),
    };

    (clocks, gpio, ethernet)
}

pub fn setup_pins(
    gpio: Gpio,
) -> EthPins<PA1<Input>, PA7<Input>, PG11<Input>, PG13<Input>, PB13<Input>, PC4<Input>, PC5<Input>>
{
    let Gpio {
        gpioa,
        gpiob,
        gpioc,
        gpiog,
    } = gpio;

    let ref_clk = gpioa.pa1.into_floating_input();
    let crs = gpioa.pa7.into_floating_input();
    let tx_d1 = gpiob.pb13.into_floating_input();
    let rx_d0 = gpioc.pc4.into_floating_input();
    let rx_d1 = gpioc.pc5.into_floating_input();

    let (tx_en, tx_d0) = {
        (
            gpiog.pg11.into_floating_input(),
            gpiog.pg13.into_floating_input(),
        )
    };

    EthPins {
        ref_clk,
        crs,
        tx_en,
        tx_d0,
        tx_d1,
        rx_d0,
        rx_d1,
    }
}
