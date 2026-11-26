//! Simulated devices (`GIZLET_FAKE_DEVICES=1`).

use super::*;
use std::thread::sleep;
use std::time::Instant;

pub fn devices() -> Devices {
    Devices {
        battery: Box::new(FakeBattery { start: Instant::now() }),
        backlight: Ok(Box::new(FakeLevel(0.7))),
        volume: Ok(Box::new(FakeLevel(0.45))),
        wifi: Ok(Box::new(FakeWifi { on: true, connected: Some("Gizlet Lab".into()), known: vec!["Gizlet Lab".into()] })),
        ssh: Ok(Box::new(FakeSsh { on: false, password: None })),
        clock: Ok(Box::new(FakeClock { zone: "Asia/Shanghai".into() })),
        host: Some(Box::new(FakeHost { name: None })),
        power: Some(Box::new(FakePower)),
        simulated: true,
        load: LoadMeter::default(),
    }
}

struct FakeBattery {
    start: Instant,
}

impl BatterySource for FakeBattery {
    fn read(&mut self) -> Result<Battery, Missing> {
        let drained = (self.start.elapsed().as_secs() / 60) as u8;
        let percent = 82u8.saturating_sub(drained).max(5);
        if std::env::var("GIZLET_FAKE_CHARGING").is_ok_and(|v| v == "1") {
            return Ok(Battery { percent, charge: Charge::Charging, power_mw: Some(3500), minutes_left: None });
        }
        // A 3000 mAh, 3.7 V battery at 1.6 W.
        let minutes_left = Some(percent as u32 * 30 * 37 * 60 / 16_000);
        Ok(Battery { percent, charge: Charge::Discharging, power_mw: Some(1600), minutes_left })
    }
}

struct FakeLevel(f32);

impl Level for FakeLevel {
    fn get(&mut self) -> Option<f32> {
        Some(self.0)
    }
    fn set(&mut self, value: f32) -> Result<(), String> {
        self.0 = value;
        Ok(())
    }
}

struct FakeWifi {
    on: bool,
    connected: Option<String>,
    known: Vec<String>,
}

const NETWORKS: [(&str, u8, bool); 6] = [
    ("Gizlet Lab", 92, true),
    ("Coffee House", 74, false),
    ("TP-LINK_5G_8A2C", 61, true),
    ("小米路由器", 48, true),
    ("Guest", 33, false),
    ("ChinaNet-xY7p", 18, true),
];

impl Wifi for FakeWifi {
    fn state(&mut self) -> Result<WifiState, String> {
        let signal = self.connected.as_ref().and_then(|c| NETWORKS.iter().find(|n| n.0 == c).map(|n| n.1));
        Ok(WifiState {
            on: self.on,
            adapter: true,
            link: if self.on { WifiLink { ssid: self.connected.clone(), signal } } else { WifiLink::default() },
        })
    }

    fn power(&mut self, on: bool) -> Result<(), String> {
        sleep(Duration::from_millis(600));
        self.on = on;
        Ok(())
    }

    fn scan(&mut self) -> Result<Vec<WifiNetwork>, String> {
        if !self.on {
            return Err("Wi-Fi is off".into());
        }
        sleep(Duration::from_millis(1200));
        Ok(NETWORKS
            .iter()
            .map(|&(ssid, signal, secured)| WifiNetwork {
                ssid: ssid.into(),
                signal,
                secured,
                known: self.known.iter().any(|k| k == ssid),
            })
            .collect())
    }

    fn connect(&mut self, ssid: &str, passphrase: Option<&str>) -> Result<(), String> {
        sleep(Duration::from_millis(1500));
        let secured = NETWORKS.iter().find(|n| n.0 == ssid).is_none_or(|n| n.2);
        let known = self.known.iter().any(|k| k == ssid);
        if secured && !known && passphrase.is_none_or(|p| p.len() < 8) {
            return Err("invalid passphrase".into());
        }
        self.connected = Some(ssid.into());
        if !known {
            self.known.push(ssid.into());
        }
        Ok(())
    }

    fn disconnect(&mut self) -> Result<(), String> {
        self.connected = None;
        Ok(())
    }

    fn forget(&mut self, ssid: &str) -> Result<(), String> {
        self.known.retain(|k| k != ssid);
        if self.connected.as_deref() == Some(ssid) {
            self.connected = None;
        }
        Ok(())
    }
}

struct FakeSsh {
    on: bool,
    password: Option<String>,
}

impl Ssh for FakeSsh {
    fn state(&mut self) -> Result<SshState, String> {
        Ok(SshState { on: self.on, running: self.on, keys: self.on.then_some(1), password: self.password.clone() })
    }
    fn set(&mut self, on: bool) -> Result<(), String> {
        sleep(Duration::from_millis(500));
        self.on = on;
        Ok(())
    }
    fn set_password(&mut self, password: &str) -> Result<(), String> {
        self.password = Some(password.into());
        Ok(())
    }
}

/// Remembers the zone; the clock shown stays in the computer's own zone.
struct FakeClock {
    zone: String,
}

impl Clock for FakeClock {
    fn state(&mut self) -> Result<ClockState, String> {
        Ok(ClockState { zone: self.zone.clone(), synced: true })
    }
    fn set_zone(&mut self, zone: &str) -> Result<(), String> {
        self.zone = zone.into();
        Ok(())
    }
}

/// Remembers the name; the default is the one from a simulated MAC address.
struct FakeHost {
    name: Option<String>,
}

impl Host for FakeHost {
    fn name(&mut self) -> String {
        self.name.clone().unwrap_or_else(|| "gizlet-a1b2".into())
    }
    fn set_name(&mut self, name: &str) -> Result<(), String> {
        if !name.is_empty() && !valid_hostname(name) {
            return Err(format!("invalid name {name}"));
        }
        self.name = Some(name.to_string()).filter(|n| !n.is_empty());
        Ok(())
    }
}

struct FakePower;

impl Power for FakePower {
    fn actions(&self) -> Vec<PowerAction> {
        vec![PowerAction::Sleep, PowerAction::Reboot, PowerAction::PowerOff]
    }
    fn run(&mut self, action: PowerAction) -> Result<(), String> {
        Err(format!("simulated {action:?}"))
    }
}
