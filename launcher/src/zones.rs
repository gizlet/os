//! The time zones offered in Settings: one or more places for each standard
//! UTC offset in use, as IANA names (files under /usr/share/zoneinfo).

use crate::settings::Language;

pub struct Zone {
    pub name: &'static str,
    /// Minutes east of UTC outside daylight saving time.
    pub offset: i32,
    en: &'static str,
    zh: &'static str,
}

const fn zone(offset: i32, name: &'static str, en: &'static str, zh: &'static str) -> Zone {
    Zone { name, offset, en, zh }
}

/// West to east.
pub const ZONES: &[Zone] = &[
    zone(-600, "Pacific/Honolulu", "Hawaii", "夏威夷"),
    zone(-540, "America/Anchorage", "Alaska", "阿拉斯加"),
    zone(-480, "America/Los_Angeles", "Pacific Time (US & Canada)", "太平洋时间（美国和加拿大）"),
    zone(-420, "America/Denver", "Mountain Time (US & Canada)", "山地时间（美国和加拿大）"),
    zone(-420, "America/Phoenix", "Arizona", "亚利桑那"),
    zone(-360, "America/Chicago", "Central Time (US & Canada)", "中部时间（美国和加拿大）"),
    zone(-360, "America/Mexico_City", "Mexico City", "墨西哥城"),
    zone(-300, "America/New_York", "Eastern Time (US & Canada)", "东部时间（美国和加拿大）"),
    zone(-300, "America/Bogota", "Bogotá, Lima", "波哥大、利马"),
    zone(-240, "America/Halifax", "Atlantic Time (Canada)", "大西洋时间（加拿大）"),
    zone(-240, "America/Caracas", "Caracas", "加拉加斯"),
    zone(-210, "America/St_Johns", "Newfoundland", "纽芬兰"),
    zone(-180, "America/Sao_Paulo", "São Paulo", "圣保罗"),
    zone(-180, "America/Argentina/Buenos_Aires", "Buenos Aires", "布宜诺斯艾利斯"),
    zone(-60, "Atlantic/Azores", "Azores", "亚速尔群岛"),
    zone(0, "Etc/UTC", "Coordinated Universal Time", "协调世界时"),
    zone(0, "Europe/London", "London, Dublin, Lisbon", "伦敦、都柏林、里斯本"),
    zone(60, "Europe/Paris", "Paris, Berlin, Madrid, Rome", "巴黎、柏林、马德里、罗马"),
    zone(60, "Africa/Lagos", "West Central Africa", "中非西部"),
    zone(120, "Europe/Athens", "Athens, Helsinki, Kyiv", "雅典、赫尔辛基、基辅"),
    zone(120, "Africa/Cairo", "Cairo", "开罗"),
    zone(120, "Africa/Johannesburg", "Johannesburg", "约翰内斯堡"),
    zone(180, "Europe/Istanbul", "Istanbul", "伊斯坦布尔"),
    zone(180, "Europe/Moscow", "Moscow", "莫斯科"),
    zone(180, "Asia/Riyadh", "Riyadh", "利雅得"),
    zone(210, "Asia/Tehran", "Tehran", "德黑兰"),
    zone(240, "Asia/Dubai", "Dubai", "迪拜"),
    zone(270, "Asia/Kabul", "Kabul", "喀布尔"),
    zone(300, "Asia/Karachi", "Karachi", "卡拉奇"),
    zone(300, "Asia/Tashkent", "Tashkent", "塔什干"),
    zone(330, "Asia/Kolkata", "India", "印度"),
    zone(345, "Asia/Kathmandu", "Kathmandu", "加德满都"),
    zone(360, "Asia/Dhaka", "Dhaka", "达卡"),
    zone(390, "Asia/Yangon", "Yangon", "仰光"),
    zone(420, "Asia/Bangkok", "Bangkok, Hanoi", "曼谷、河内"),
    zone(420, "Asia/Jakarta", "Jakarta", "雅加达"),
    zone(480, "Asia/Shanghai", "Beijing, Shanghai", "北京、上海"),
    zone(480, "Asia/Hong_Kong", "Hong Kong", "香港"),
    zone(480, "Asia/Taipei", "Taipei", "台北"),
    zone(480, "Asia/Singapore", "Singapore, Kuala Lumpur", "新加坡、吉隆坡"),
    zone(480, "Australia/Perth", "Perth", "珀斯"),
    zone(540, "Asia/Tokyo", "Tokyo", "东京"),
    zone(540, "Asia/Seoul", "Seoul", "首尔"),
    zone(570, "Australia/Adelaide", "Adelaide", "阿德莱德"),
    zone(600, "Australia/Brisbane", "Brisbane", "布里斯班"),
    zone(600, "Australia/Sydney", "Sydney, Melbourne", "悉尼、墨尔本"),
    zone(720, "Pacific/Auckland", "Auckland", "奥克兰"),
];

impl Zone {
    pub fn find(name: &str) -> Option<&'static Zone> {
        ZONES.iter().find(|z| z.name == name || (z.name == "Etc/UTC" && matches!(name, "UTC" | "Etc/Universal" | "Universal")))
    }

    /// Such as `UTC+08:00 Beijing, Shanghai`.
    pub fn label(&self, lang: Language) -> String {
        let place = if lang == Language::ZhCn { self.zh } else { self.en };
        format!("{} {place}", utc_offset(self.offset))
    }
}

/// `UTC+05:30`, `UTC-03:00`, or `UTC` for zero.
pub fn utc_offset(minutes: i32) -> String {
    if minutes == 0 {
        return "UTC".into();
    }
    let sign = if minutes < 0 { '-' } else { '+' };
    let m = minutes.abs();
    format!("UTC{sign}{:02}:{:02}", m / 60, m % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_and_order() {
        assert_eq!(Zone::find("Asia/Shanghai").unwrap().label(Language::En), "UTC+08:00 Beijing, Shanghai");
        assert_eq!(Zone::find("Asia/Kolkata").unwrap().label(Language::ZhCn), "UTC+05:30 印度");
        assert_eq!(utc_offset(-210), "UTC-03:30");
        assert_eq!(Zone::find("UTC").unwrap().name, "Etc/UTC");
        assert!(ZONES.windows(2).all(|w| w[0].offset <= w[1].offset));
        let mut names: Vec<_> = ZONES.iter().map(|z| z.name).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), ZONES.len());
    }

    /// Every name is a zone file where the system has a zone database.
    #[test]
    fn zones_exist() {
        let dir = std::path::Path::new("/usr/share/zoneinfo");
        if dir.is_dir() {
            for z in ZONES {
                assert!(dir.join(z.name).is_file(), "{} is missing", z.name);
            }
        }
    }
}
