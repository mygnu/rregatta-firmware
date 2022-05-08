#![deny(unsafe_code)]
// #![deny(warnings)]
#![no_main]
#![no_std]

use rregatta32 as _;

#[rtic::app(device = stm32f1xx_hal::pac, dispatchers = [RTC, TIM2])]
mod app {
    use defmt::{println, Format};
    use oorandom::Rand64;
    use stm32f1xx_hal::{
        gpio::{
            gpiob::{PB0, PB1, PB10, PB11, PB12, PB13},
            GpioExt, Input, Output, PullDown, PushPull,
        },
        prelude::*,
        rcc::RccExt,
    };
    use systick_monotonic::{fugit::TimerInstantU64, ExtU64, Systick};

    // A monotonic timer to enable scheduling in RTIC
    #[monotonic(binds = SysTick, default = true)]
    type MonotonicTick = Systick<5000>; // 5000 Hz / 200 µs granularity

    // shared resources between tasks
    // each resource can be passed to a task selectively
    #[shared]
    struct Shared {
        horn: PB0<Output<PushPull>>,    // 18
        light1: PB11<Output<PushPull>>, // 22
        light2: PB10<Output<PushPull>>, // 21
        light3: PB1<Output<PushPull>>,  // 19
        handel: Option<controller::MonotonicTick::SpawnHandle>,
    }

    // one minute in seconds
    const ONE_MINUTE_S: u64 = 60;

    #[local]
    struct Local {
        start_button: PB12<Input<PullDown>>, // 25 5V/IO
        stop_button: PB13<Input<PullDown>>,  // 26 5V/IO
    }

    #[init]
    fn init(cx: init::Context) -> (Shared, Local, init::Monotonics) {
        let dp = cx.device; // device peripherals
        let mut flash = dp.FLASH.constrain();
        let rcc = dp.RCC.constrain();
        // Acquire the GPIOB peripheral
        let mut gpiob = dp.GPIOB.split();

        let _clocks = rcc
            .cfgr
            .use_hse(8.MHz())
            .sysclk(32.MHz())
            .freeze(&mut flash.acr);

        // Initialize the monotonic clock based on system timer running at 32Mhz
        // (see _clocks)
        let mut mono = Systick::new(cx.core.SYST, 32_000_000);

        let start_button = gpiob.pb12.into_pull_down_input(&mut gpiob.crh);
        let stop_button = gpiob.pb13.into_pull_down_input(&mut gpiob.crh);

        let horn = gpiob.pb0.into_push_pull_output(&mut gpiob.crl);
        let light1 = gpiob.pb11.into_push_pull_output(&mut gpiob.crh);
        let light2 = gpiob.pb10.into_push_pull_output(&mut gpiob.crh);
        let light3 = gpiob.pb1.into_push_pull_output(&mut gpiob.crl);

        reset_all::spawn().unwrap();

        // spawn task to periodically check button state
        poll_buttons::spawn(mono.now()).unwrap();

        (
            Shared {
                handel: None,
                horn,
                light1,
                light2,
                light3,
            },
            Local {
                start_button,
                stop_button,
            },
            init::Monotonics(mono),
        )
    }

    /// periodic task to check buttons
    #[task(priority=2, local = [count: u64 = 0, start_button, stop_button], shared = [handel])]
    fn poll_buttons(
        mut cx: poll_buttons::Context,
        instant: TimerInstantU64<5000>,
    ) {
        let poll_buttons::LocalResources {
            start_button,
            stop_button,
            count,
        } = cx.local;

        // up the tick count by one
        *count = count.wrapping_add(1);

        cx.shared.handel.lock(|handel| {
            if stop_button.is_high() {
               if let Some(h) = handel.take() {
                   defmt::println!("Stopping");
                   reset_all::spawn().ok();
                   if h.cancel().is_ok() {
                       defmt::println!("stopped");
                       beep_horn::spawn_after(100_u64.millis(), 300, 2).ok();
                   } else {
                       defmt::println!("Something went wrong");
                   }
               }
           } else if start_button.is_high() && handel.is_none() {
               defmt::println!("spawning");
               *handel = controller::spawn_at(
                   monotonics::now(),
                   instant,
                   State::Warmup,
                   *count,
               )
               .ok();
           } 
       });
        // Periodic check buttons every 50ms
        poll_buttons::spawn_at(instant, instant + 50_u64.millis()).unwrap();
    }

    /// State of the race timer each variant is used to perform a specific
    /// operation and trigger next next task with a new state.
    #[derive(Debug, Clone, Copy, Format)]
    pub enum State {
        Warmup,
        Three,
        Two,
        One,
        Start,
    }

    #[task(priority=1, shared = [handel])]
    fn controller(
        mut cx: controller::Context,
        instant: TimerInstantU64<5000>,
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
                    controller::spawn_at(
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
            Warmup => {
                // horn for 800ms once
                beep_horn::spawn(800, 1).ok();

                defmt::println!("Seed {}", seed);
                let random = Rand64::new(seed.into()).rand_range(30..60);
                defmt::println!("Warmup period: {}secs", random);

                re_spawn(Three, random);
            }
            Three => {
                beep_horn::spawn(1200, 1).ok();
                set_lights::spawn_after(
                    100_u64.millis(),
                    Light::On,
                    Light::On,
                    Light::On,
                )
                .ok();
                re_spawn(Two, ONE_MINUTE_S);
            }
            Two => {
                beep_horn::spawn(400, 1).ok();
                set_lights::spawn_after(
                    100_u64.millis(),
                    Light::Off,
                    Light::On,
                    Light::On,
                )
                .ok();
                re_spawn(One, ONE_MINUTE_S);
            }
            One => {
                beep_horn::spawn(400, 1).ok();
                set_lights::spawn(Light::Off, Light::Off, Light::On).ok();
                re_spawn(Start, ONE_MINUTE_S);
            }
            Start => {
                beep_horn::spawn(2000, 1).ok();
                set_lights::spawn(Light::Off, Light::Off, Light::Off).ok();
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

    #[derive(Format, Debug)]
    pub enum Light {
        On,
        Off,
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
    #[task(priority=1, local = [is_high: bool = false], shared = [horn])]
    fn beep_horn(mut cx: beep_horn::Context, millis: u64, repetition: i8) {
        if !*cx.local.is_high {
            *cx.local.is_high = true;
            cx.shared.horn.lock(|horn| {
                println!("horn START");
                horn.set_high();
            });
            beep_horn::spawn_after(millis.millis(), millis, repetition - 1)
                .ok();
        } else {
            *cx.local.is_high = false;
            cx.shared.horn.lock(|horn| {
                println!("horn STOP");
                horn.set_low();
            });
            // spawn again if repetitions are left
            if repetition > 0 {
                beep_horn::spawn_after(50_u64.millis(), millis, repetition - 1)
                    .ok();
            }
        }
    }

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
