#![deny(unsafe_code)]
// #![deny(warnings)]
#![no_main]
#![no_std]

use regatta32 as _;

#[rtic::app(device = stm32f1xx_hal::pac, dispatchers = [RTC, TIM2])]
mod app {

    use cortex_m::asm::delay;
    use defmt::{println, Format};

    use stm32f1xx_hal::{
        gpio::{
            gpiob::{PB0, PB1, PB10, PB11, PB12, PB13},
            GpioExt, Input, Output, PullDown, PushPull,
        },
        prelude::*,
        rcc::RccExt,
    };
    use systick_monotonic::*;

    #[monotonic(binds = SysTick, default = true)]
    type MyMono = Systick<5000>; // 1000 Hz / 1 ms granularity

    #[shared]
    struct Shared {
        // led: PC13<Output<PushPull>>,
        start_button: PB12<Input<PullDown>>, // 25 FV/IO
        stop_button: PB13<Input<PullDown>>,  // 26 FV/IO
        horn: PB0<Output<PushPull>>,         // 22
        light1: PB11<Output<PushPull>>,
        light2: PB10<Output<PushPull>>,
        light3: PB1<Output<PushPull>>,
        handel: Option<machine::MyMono::SpawnHandle>,
    }

    // const WARNING_PERIOD: u64 = 30;
    const ONE_MINUTE: u64 = 60;

    #[local]
    struct Local {}

    #[init]
    fn init(cx: init::Context) -> (Shared, Local, init::Monotonics) {
        let dp = cx.device;
        let mut flash = dp.FLASH.constrain();
        let rcc = dp.RCC.constrain();
        // Acquire the GPIOC peripheral
        // let mut gpioc = dp.GPIOC.split();
        let mut gpiob = dp.GPIOB.split();

        let _clocks = rcc
            .cfgr
            .use_hse(8.mhz())
            .sysclk(32.mhz())
            .freeze(&mut flash.acr);

        let systick = cx.core.SYST;

        // Initialize the monotonic
        let mut mono = Systick::new(systick, 32_000_000);

        let start_button = gpiob.pb12.into_pull_down_input(&mut gpiob.crh);
        let stop_button = gpiob.pb13.into_pull_down_input(&mut gpiob.crh);

        let horn = gpiob.pb0.into_push_pull_output(&mut gpiob.crl);
        let light1 = gpiob.pb11.into_push_pull_output(&mut gpiob.crh);
        let light2 = gpiob.pb10.into_push_pull_output(&mut gpiob.crh);
        let light3 = gpiob.pb1.into_push_pull_output(&mut gpiob.crl);

        reset_all::spawn().ok();

        check_buttons::spawn(mono.now()).unwrap();
        // led.set_low();

        (
            Shared {
                // led,
                start_button,
                stop_button,
                handel: None,
                horn,
                light1,
                light2,
                light3,
            },
            Local {},
            init::Monotonics(mono),
        )
    }

    enum Action {
        None,
        Start,
        Stop,
    }

    #[task(priority=2, local = [count: u64 = 0], shared = [start_button, stop_button, handel])]
    fn check_buttons(
        mut cx: check_buttons::Context,
        instant: fugit::TimerInstantU64<5000>,
    ) {
        let instant = instant + 50.millis();
        let seed = *cx.local.count + 1;
        *cx.local.count = seed;
        let mut action = Action::None;
        cx.shared.start_button.lock(|btn| {
            if btn.is_high() {
                action = Action::Start;
            }
        });
        cx.shared.stop_button.lock(|btn| {
            if btn.is_high() {
                action = Action::Stop;
            }
        });

        match action {
            Action::Start => {
                cx.shared.handel.lock(|handel| {
                    if handel.is_none() {
                        defmt::println!("spawning");
                        *handel = Some(
                            machine::spawn_at(
                                monotonics::now(),
                                instant,
                                State::Warning,
                                seed,
                            )
                            .unwrap(),
                        )
                    }
                });
            }
            Action::Stop => {
                cx.shared.handel.lock(|handel| {
                    if let Some(h) = handel.take() {
                        defmt::println!("Stopping");
                        reset_all::spawn().ok();
                        if h.cancel().is_ok() {
                            defmt::println!("stopped");
                        } else {
                            defmt::println!("not stopped");
                        }
                    }
                });
            }
            _ => {}
        }

        // Periodic ever 1 seconds
        check_buttons::spawn_at(instant, instant).unwrap();
    }

    #[derive(Debug, Clone, Copy, Format)]
    pub enum State {
        Warning,
        Three,
        Two,
        One,
        Start,
    }

    #[task(priority=1, shared = [handel])]
    fn machine(
        mut cx: machine::Context,
        instant: fugit::TimerInstantU64<5000>,
        state: State,
        seed: u64,
    ) {
        use State::*;

        defmt::println!("State {:?}", state);

        // re-spawn self with given state and time (seconds from now)
        let mut re_spawn = |state: State, secs: u64| {
            cx.shared.handel.lock(|handel| {
                defmt::println!("spawning {:?}", state);
                *handel = Some(
                    machine::spawn_at(
                        instant + secs.secs(),
                        instant + secs.secs(),
                        state,
                        seed,
                    )
                    .unwrap(),
                )
            });
        };

        match state {
            Warning => {
                // horn for 500ms
                beep_horn::spawn(500).unwrap();

                defmt::println!("Seed {}", seed);
                let random =
                    oorandom::Rand64::new(seed.into()).rand_range(40..90);
                defmt::println!("Warning period: {}secs", random);

                re_spawn(Three, random);
            }
            Three => {
                beep_horn::spawn(1200).unwrap();
                set_lights::spawn_after(50.millis(), true, true, true).unwrap();
                // horn for 1200ms
                re_spawn(Two, ONE_MINUTE);
            }
            Two => {
                set_lights::spawn(false, true, true).unwrap();
                beep_horn::spawn(200).unwrap();
                re_spawn(One, ONE_MINUTE);
            }
            One => {
                set_lights::spawn(false, false, true).unwrap();
                beep_horn::spawn(200).unwrap();
                re_spawn(Start, ONE_MINUTE);
            }
            Start => {
                beep_horn::spawn(2000).unwrap();
                set_lights::spawn(false, false, false).unwrap();
                defmt::println!("Start !!!!!!!!!!!!!!");
                cx.shared.handel.lock(|handel| *handel = None);
                // let next_instant = instant + 1.secs();
                // cx.shared.handel.lock(|handel| {
                //     defmt::println!("spawning {:?}", state);
                //     do_start::spawn().unwrap();
                //     *handel = Some(
                //         machine::spawn_at(next_instant, next_instant, Begin)
                //             .unwrap(),
                //     )
                // });
            }
        }
    }

    #[task(priority=1, shared = [horn, light1, light2, light3])]
    fn reset_all(cx: reset_all::Context) {
        let reset_all::SharedResources {
            horn,
            light1,
            light2,
            light3,
        } = cx.shared;

        (horn, light1, light2, light3).lock(|horn, light1, light2, light3| {
            defmt::println!("Reset all");
            horn.set_low();
            light1.set_low();
            light2.set_low();
            light3.set_low();
        });
    }

    /// set light status with a small delay in between
    #[task(priority=1, shared = [light1, light2, light3])]
    fn set_lights(cx: set_lights::Context, l1: bool, l2: bool, l3: bool) {
        let set_lights::SharedResources {
            light1,
            light2,
            light3,
        } = cx.shared;

        (light1, light2, light3).lock(|light1, light2, light3| {
            defmt::println!("Setting lights 1:{} 2:{} 3:{}", l1, l2, l3);

            if l1 {
                light1.set_high();
            } else {
                light1.set_low();
            }
            delay(1000);
            if l2 {
                light2.set_high();
            } else {
                light2.set_low();
            }
            delay(1000);
            if l3 {
                light3.set_high();
            } else {
                light3.set_low();
            }
        });
    }

    /// set horn state high for given milliseconds
    #[task(priority=1, local = [started: bool = false], shared = [horn])]
    fn beep_horn(mut cx: beep_horn::Context, millis: u64) {
        if !*cx.local.started {
            *cx.local.started = true;
            cx.shared.horn.lock(|horn| {
                println!("horn START");
                horn.set_high();
            });
            beep_horn::spawn_after(millis.millis(), millis).ok();
        } else {
            *cx.local.started = false;
            cx.shared.horn.lock(|horn| {
                println!("horn STOP");
                horn.set_low();
            })
        }
    }

    // Optional.
    //
    // https://rtic.rs/dev/book/en/by-example/app_idle.html
    // > When no idle function is declared, the runtime sets the SLEEPONEXIT bit and then
    // > sends the microcontroller to sleep after running init.
    #[idle]
    fn idle(_cx: idle::Context) -> ! {
        loop {
            cortex_m::asm::nop();
        }
    }
}
