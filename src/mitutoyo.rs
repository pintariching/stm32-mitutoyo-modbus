use stm32f4xx_hal::{
    gpio::{Input, Output, Pin},
    hal_02::can::nb,
    prelude::*,
    timer::{SysCounter, SysEvent},
};

use crate::TIME;

pub struct Urica<
    const DP: char,
    const DN: u8,
    const CP: char,
    const CN: u8,
    const RP: char,
    const RN: u8,
    const OP: char,
    const ON: u8,
> {
    data: Pin<DP, DN, Input>,
    clock: Pin<CP, CN, Input>,
    request: Pin<RP, RN, Output>,
    origin: Pin<OP, ON, Output>,
}

impl<
        const DP: char,
        const DN: u8,
        const CP: char,
        const CN: u8,
        const RP: char,
        const RN: u8,
        const OP: char,
        const ON: u8,
    > Urica<DP, DN, CP, CN, RP, RN, OP, ON>
{
    pub fn new(
        data: Pin<DP, DN, Input>,
        clock: Pin<CP, CN, Input>,
        request: Pin<RP, RN, Output>,
        origin: Pin<OP, ON, Output>,
    ) -> Self {
        Self {
            data,
            clock,
            request,
            origin,
        }
    }

    pub fn measure(&mut self) -> Option<[u8; 13]> {
        let timeout = 40;
        let mut data = [0u8; 13];

        self.request.set_high();

        for i in 0..13 {
            let mut k = 0u8;

            for _ in 0..4 {
                let start_time = cortex_m::interrupt::free(|cs| *TIME.borrow(cs).borrow());
                while self.clock.is_low() {
                    let current_time = cortex_m::interrupt::free(|cs| *TIME.borrow(cs).borrow());

                    if current_time - start_time > timeout {
                        return None;
                    }
                }

                let start_time = cortex_m::interrupt::free(|cs| *TIME.borrow(cs).borrow());
                while self.clock.is_high() {
                    let current_time = cortex_m::interrupt::free(|cs| *TIME.borrow(cs).borrow());

                    if current_time - start_time > timeout {
                        return None;
                    }
                }

                let bit = self.data.is_high() as u8 & 0b0000_0001;
                k <<= 1;
                k = k & bit;
            }

            if i == 0 {
                self.request.set_low();
            }

            data[i] = k
        }

        Some(data)
    }
}
