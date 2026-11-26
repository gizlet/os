//! macOS and Windows: the system manages the screen, sound and network; the
//! launcher only reads the battery.

use super::*;

pub fn devices() -> Devices {
    Devices {
        battery: Box::new(SystemBattery),
        backlight: Err(Missing::ManagedBySystem),
        volume: Err(Missing::ManagedBySystem),
        wifi: Err(Missing::ManagedBySystem),
        ssh: Err(Missing::ManagedBySystem),
        clock: Err(Missing::ManagedBySystem),
        power: None,
        simulated: false,
        load: LoadMeter::default(),
    }
}

struct SystemBattery;

#[cfg(target_os = "macos")]
impl BatterySource for SystemBattery {
    /// `pmset -g batt` prints, for example, ` -InternalBattery-0 (id=…)	82%; discharging; 4:12 remaining`.
    fn read(&mut self) -> Result<Battery, Missing> {
        let out = util::run("pmset", &["-g", "batt"]).map_err(Missing::Error)?;
        let line = out.lines().find(|l| l.contains("InternalBattery")).ok_or(Missing::NoHardware)?;
        let fields: Vec<&str> = line.split('\t').nth(1).unwrap_or("").split(';').map(str::trim).collect();
        let percent = fields
            .first()
            .and_then(|f| f.trim_end_matches('%').parse::<u8>().ok())
            .ok_or_else(|| Missing::Error("unreadable".into()))?;
        let charge = match fields.get(1).copied() {
            Some("charging") => Charge::Charging,
            Some("charged") | Some("finishing charge") | Some("AC attached") => Charge::Full,
            _ => Charge::Discharging,
        };
        let minutes_left = match charge {
            Charge::Discharging => fields.get(2).and_then(|f| pmset_minutes_left(f)),
            _ => None,
        };
        Ok(Battery { percent: percent.min(100), charge, power_mw: None, minutes_left })
    }
}

/// The third field of `pmset -g batt`: "4:12 remaining", often followed by
/// " present: true", or "(no estimate)" while macOS is still measuring.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn pmset_minutes_left(field: &str) -> Option<u32> {
    let mut words = field.split_whitespace();
    let (h, m) = words.next()?.split_once(':')?;
    if words.next()? != "remaining" {
        return None;
    }
    Some(h.parse::<u32>().ok()? * 60 + m.parse::<u32>().ok()?)
}

#[cfg(test)]
mod tests {
    use super::pmset_minutes_left;

    #[test]
    fn reads_pmset_estimates() {
        assert_eq!(pmset_minutes_left("4:12 remaining"), Some(252));
        assert_eq!(pmset_minutes_left("4:12 remaining present: true"), Some(252));
        assert_eq!(pmset_minutes_left("0:05 remaining present: true"), Some(5));
        assert_eq!(pmset_minutes_left("(no estimate)"), None);
        assert_eq!(pmset_minutes_left("(no estimate) present: true"), None);
        assert_eq!(pmset_minutes_left("4:12"), None);
    }
}

#[cfg(windows)]
#[repr(C)]
struct SystemPowerStatus {
    ac_line_status: u8,
    battery_flag: u8,
    battery_life_percent: u8,
    system_status_flag: u8,
    battery_life_time: u32,
    battery_full_life_time: u32,
}

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn GetSystemPowerStatus(status: *mut SystemPowerStatus) -> i32;
}

#[cfg(windows)]
impl BatterySource for SystemBattery {
    fn read(&mut self) -> Result<Battery, Missing> {
        let mut s = std::mem::MaybeUninit::<SystemPowerStatus>::zeroed();
        if unsafe { GetSystemPowerStatus(s.as_mut_ptr()) } == 0 {
            return Err(Missing::Error("unreadable".into()));
        }
        let s = unsafe { s.assume_init() };
        // 128: no system battery; 255: unknown.
        if s.battery_flag & 128 != 0 || s.battery_life_percent > 100 {
            return Err(Missing::NoHardware);
        }
        let charge = match (s.ac_line_status, s.battery_flag & 8 != 0) {
            (_, true) => Charge::Charging,
            (1, false) => Charge::Full,
            _ => Charge::Discharging,
        };
        // Seconds, or u32::MAX when unknown or on AC power.
        let minutes_left = (charge == Charge::Discharging && s.battery_life_time != u32::MAX).then(|| s.battery_life_time / 60);
        Ok(Battery { percent: s.battery_life_percent, charge, power_mw: None, minutes_left })
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
impl BatterySource for SystemBattery {
    fn read(&mut self) -> Result<Battery, Missing> {
        Err(Missing::NoHardware)
    }
}
