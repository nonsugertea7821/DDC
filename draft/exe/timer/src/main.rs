mod types;
use types::{Alarm, Timer};

include!(concat!(env!("OUT_DIR"), "/ddc_generated.rs"));

fn main() {
    let mut timer = new_timer();
    let mut alarm5 = new_alarm("5ms alarm".to_string(), 5);
    let mut alarm8 = new_alarm("8ms alarm".to_string(), 8);

    println!("=== DDC Timer Demo ===");
    println!("Alarm 1: '{}' fires at {}ms", alarm5.label, alarm5.threshold_ms);
    println!("Alarm 2: '{}' fires at {}ms", alarm8.label, alarm8.threshold_ms);
    println!();

    for i in 1..=10 {
        timer = tick(timer);
        alarm5 = check_alarm(timer.clone(), alarm5);
        alarm8 = check_alarm(timer.clone(), alarm8);
        println!(
            "tick {:2}: elapsed={:3}ms | {} | {}",
            i,
            timer.elapsed_ms,
            if alarm5.fired { "[FIRED] alarm1" } else { "       alarm1" },
            if alarm8.fired { "[FIRED] alarm2" } else { "       alarm2" },
        );
    }

    println!();
    println!("alarm_label demo: '{}'", alarm_label(alarm5.clone()));

    // IAlarmService trait のメソッド呼び出し (impl SimpleAlarmService)
    let svc = SimpleAlarmService;
    let result = svc.check(timer.clone(), alarm8.clone());
    println!("IAlarmService::check(elapsed={}, threshold={}) = {}", timer.elapsed_ms, alarm8.threshold_ms, result);

    println!();
    println!("Resetting...");
    timer = reset_timer(timer);
    alarm5 = reset_alarm(alarm5);
    println!("elapsed after reset: {}ms, alarm1 fired: {}", timer.elapsed_ms, alarm5.fired);
}
