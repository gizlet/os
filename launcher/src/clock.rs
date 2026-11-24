//! Local time for the status bar, from the C library or Windows.

#[derive(Clone, Copy, Debug, Default)]
pub struct Now {
    pub month: u32,
    pub day: u32,
    /// 0 is Monday.
    pub weekday: u32,
    pub hour: u32,
    pub minute: u32,
}

#[cfg(unix)]
pub fn now() -> Now {
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&t, &mut tm).is_null() {
            return Now::default();
        }
        Now {
            month: tm.tm_mon as u32 + 1,
            day: tm.tm_mday as u32,
            weekday: (tm.tm_wday as u32 + 6) % 7,
            hour: tm.tm_hour as u32,
            minute: tm.tm_min as u32,
        }
    }
}

#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct SystemTime {
    year: u16,
    month: u16,
    day_of_week: u16,
    day: u16,
    hour: u16,
    minute: u16,
    second: u16,
    milliseconds: u16,
}

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn GetLocalTime(time: *mut SystemTime);
}

#[cfg(windows)]
pub fn now() -> Now {
    let mut t = SystemTime::default();
    unsafe { GetLocalTime(&mut t) };
    Now {
        month: t.month as u32,
        day: t.day as u32,
        weekday: (t.day_of_week as u32 + 6) % 7,
        hour: t.hour as u32,
        minute: t.minute as u32,
    }
}

const WEEKDAYS_EN: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const WEEKDAYS_ZH: [&str; 7] = ["一", "二", "三", "四", "五", "六", "日"];
const MONTHS_EN: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

impl Now {
    pub fn date(self, chinese: bool) -> String {
        let wd = self.weekday.min(6) as usize;
        if chinese {
            format!("{}月{}日 星期{}", self.month, self.day, WEEKDAYS_ZH[wd])
        } else {
            let month = MONTHS_EN[(self.month.clamp(1, 12) - 1) as usize];
            format!("{}, {} {}", WEEKDAYS_EN[wd], month, self.day)
        }
    }

    pub fn time(self, h24: bool) -> String {
        if h24 {
            format!("{:02}:{:02}", self.hour, self.minute)
        } else {
            let h = match self.hour % 12 {
                0 => 12,
                h => h,
            };
            format!("{}:{:02} {}", h, self.minute, if self.hour < 12 { "AM" } else { "PM" })
        }
    }
}
