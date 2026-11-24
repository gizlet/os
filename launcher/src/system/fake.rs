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
        ssh: Ok(Box::new(FakeSsh { on: false })),
        power: Some(Box::new(FakePower)),
        simulated: true,
    }
}

struct FakeBattery {
    start: Instant,
}

impl BatterySource for FakeBattery {
    fn read(&mut self) -> Result<Battery, Missing> {
        let drained = (self.start.elapsed().as_secs() / 60) as u8;
        Ok(Battery { percent: 82u8.saturating_sub(drained).max(5), charge: Charge::Discharging })
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
}

impl Ssh for FakeSsh {
    fn state(&mut self) -> Result<SshState, String> {
        Ok(SshState { on: self.on, running: self.on, keys: self.on.then_some(1) })
    }
    fn set(&mut self, on: bool) -> Result<(), String> {
        sleep(Duration::from_millis(500));
        self.on = on;
        Ok(())
    }
}

struct FakePower;

impl Power for FakePower {
    fn actions(&self) -> Vec<PowerAction> {
        vec![PowerAction::Reboot, PowerAction::PowerOff]
    }
    fn run(&mut self, action: PowerAction) -> Result<(), String> {
        Err(format!("simulated {action:?}"))
    }
}
