#![deny(unsafe_code)]
// #![deny(warnings)]
#![no_main]
#![no_std]

use regatta32 as _;

#[rtic::app(device = stm32f1xx_hal::pac, dispatchers = [RTC, TIM2])]
mod app {
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

    // A monotonic timer to enable scheduling in RTIC
    #[monotonic(binds = SysTick, default = true)]
    type MyMono = Systick<5000>; // 5000 Hz / 200 µs granularity

    // shared resources between tasks
    // each resource can be passed to a task selectively
    #[shared]
    struct Shared {
        start_button: PB12<Input<PullDown>>, // 25 5V/IO
        stop_button: PB13<Input<PullDown>>,  // 26 5V/IO
        horn: PB0<Output<PushPull>>,         // 18
        light1: PB11<Output<PushPull>>,      // 22
        light2: PB10<Output<PushPull>>,      // 21
        light3: PB1<Output<PushPull>>,       // 19
        handel: Option<machine::MyMono::SpawnHandle>,
    }

    // one minute in seconds
    const ONE_MINUTE_S: u64 = 60;

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
        let seed = cx.local.count.wrapping_add(1);
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

    /// State of the race timer each variant is used to perform a specific operation and trigger
    /// next next task with a new state.
    #[derive(Debug, Clone, Copy, Format)]
    pub enum State {
        Warning,
        Three,
        Two,
        One,
        Start,
    }

    #[derive(Format, Debug)]
    pub enum Light {
        On,
        Off,
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
                    oorandom::Rand64::new(seed.into()).rand_range(20..60);
                defmt::println!("Warning period: {}secs", random);

                re_spawn(Three, random);
            }
            Three => {
                beep_horn::spawn(1000).unwrap();
                set_lights::spawn_after(
                    100.millis(),
                    Light::On,
                    Light::On,
                    Light::On,
                )
                .unwrap();
                re_spawn(Two, ONE_MINUTE_S);
            }
            Two => {
                beep_horn::spawn(200).unwrap();
                set_lights::spawn_after(
                    100.millis(),
                    Light::Off,
                    Light::On,
                    Light::On,
                )
                .unwrap();
                re_spawn(One, ONE_MINUTE_S);
            }
            One => {
                beep_horn::spawn(200).unwrap();
                set_lights::spawn(Light::Off, Light::Off, Light::On).unwrap();
                re_spawn(Start, ONE_MINUTE_S);
            }
            Start => {
                beep_horn::spawn(2000).unwrap();
                set_lights::spawn(Light::Off, Light::Off, Light::Off).unwrap();
                defmt::println!("Start !!!!!!!!!!!!!!");
                cx.shared.handel.lock(|handel| *handel = None);
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
    fn set_lights(cx: set_lights::Context, l1: Light, l2: Light, l3: Light) {
        let set_lights::SharedResources {
            light1,
            light2,
            light3,
        } = cx.shared;

        (light1, light2, light3).lock(|light1, light2, light3| {
            defmt::println!("Setting lights 1:{} 2:{} 3:{}", l1, l2, l3);

            match l1 {
                Light::On => light1.set_high(),
                Light::Off => light1.set_low(),
            }
            match l2 {
                Light::On => light2.set_high(),
                Light::Off => light2.set_low(),
            }
            match l3 {
                Light::On => light3.set_high(),
                Light::Off => light3.set_low(),
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
            // Now Wait For Interrupt is used instead of a busy-wait loop
            // to allow MCU to sleep between interrupts
            // https://developer.arm.com/documentation/ddi0406/c/Application-Level-Architecture/Instruction-Details/Alphabetical-list-of-instructions/WFI
            rtic::export::wfi()
        }
    }
}
