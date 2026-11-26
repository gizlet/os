//! Interface strings in Simplified Chinese and English.

use crate::settings::Language;

macro_rules! strings {
    ($($key:ident => $zh:literal, $en:literal;)*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum T { $($key),* }

        impl T {
            pub const ALL: &'static [T] = &[$(T::$key),*];

            /// The string in `lang`, which must already be resolved.
            pub fn get(self, lang: Language) -> &'static str {
                match (self, lang) {
                    $((T::$key, Language::ZhCn) => $zh, (T::$key, _) => $en,)*
                }
            }
        }
    };
}

strings! {
    Back => "返回", "Back";
    Ok => "确定", "OK";
    Select => "选择", "Select";
    Refresh => "刷新", "Refresh";
    Change => "调整", "Adjust";
    Delete => "删除", "Delete";
    Close => "关闭", "Close";

    Library => "程序库", "Library";
    Store => "商店", "Store";
    ButtonTest => "按键测试", "Button Test";
    Settings => "设置", "Settings";
    Power => "电源", "Power";
    EmptyTitle => "还没有安装程序", "No apps installed yet";
    EmptyBody => "用 gizlet install 安装的程序会出现在这里。", "Apps you install with gizlet install appear here.";
    Starting => "正在启动 {}", "Starting {}";
    StoreSoon => "商店还没有开放", "The Store isn't open yet";
    LibrarySoon => "还没有安装程序", "No apps are installed yet";

    PowerTitle => "电源选项", "Power Options";
    Sleep => "睡眠", "Sleep";
    Reboot => "重新启动", "Restart";
    PowerOff => "关机", "Power Off";
    QuitLauncher => "退出 launcher", "Quit the Launcher";

    CatNetwork => "网络", "Network";
    CatSsh => "SSH", "SSH";
    CatDisplay => "显示", "Display";
    CatSound => "声音", "Sound";
    CatBattery => "电池", "Battery";
    CatControls => "控制器", "Controls";
    CatLanguage => "语言", "Language";
    CatTime => "日期与时间", "Date & Time";
    CatSystem => "系统", "System";

    Wifi => "Wi-Fi", "Wi-Fi";
    WifiOffBody => "打开 Wi-Fi 后可以搜索和连接网络。", "Turn on Wi-Fi to find and join networks.";
    WifiStarting => "正在打开 Wi-Fi…", "Turning on Wi-Fi…";
    WifiCurrent => "当前网络", "Current Network";
    WifiNotConnected => "未连接", "Not connected";
    WifiAvailable => "可用网络", "Available Networks";
    WifiScanning => "正在搜索网络…", "Searching for networks…";
    WifiScan => "重新搜索", "Search Again";
    WifiNone => "没有找到网络", "No networks found";
    WifiConnecting => "正在连接…", "Connecting…";
    WifiConnected => "已连接", "Connected";
    WifiSaved => "已保存", "Saved";
    WifiConnectedTo => "已连接到“{}”", "Connected to \"{}\"";
    WifiConnectFailed => "无法连接到“{}”：{}", "Couldn't connect to \"{}\": {}";
    WifiDisconnect => "断开连接", "Disconnect";
    WifiForget => "忘记此网络", "Forget Network";
    WifiPassphrase => "输入“{}”的密码", "Password for \"{}\"";
    WifiNoHardware => "未检测到无线网卡", "No Wi-Fi adapter detected";
    WifiNoHardwareBody => "插入受支持的 USB 无线网卡后会自动识别。", "Plug in a supported USB Wi-Fi adapter and it will be detected.";

    Ssh => "SSH 远程登录", "SSH Remote Login";
    SshRunning => "正在运行", "Running";
    SshStopped => "未运行", "Not running";
    SshLogin => "登录地址", "Log In With";
    SshNeedsNetwork => "连接网络后显示", "Shown once connected";
    SshKeys => "公钥", "Public Keys";
    SshKeyCount => "{} 个", "{}";
    SshNoKeys => "没有公钥，无法登录", "None, logins are refused";
    SshPassword => "密码", "Password";
    SshPasswordUnset => "未设置，按 A 设置", "Not set, press A to set";
    SshPasswordTitle => "SSH 密码（用户 root）", "SSH password (user root)";
    SshPasswordSaved => "SSH 密码已更新", "SSH password updated";

    MissingService => "缺少系统组件 {}", "The {} system component is missing";
    MissingServiceBody => "这台设备的系统没有提供它，相关功能暂不可用。", "This device's system doesn't provide it, so this feature is unavailable.";
    ManagedBySystem => "由系统管理", "Managed by the system";
    ManagedBySystemBody => "在这台电脑上，请在系统设置中调整。", "On this computer, change it in the system settings.";
    DeviceError => "设备出错：{}", "Device error: {}";

    Brightness => "屏幕亮度", "Brightness";
    NoBacklight => "未检测到可调节的背光", "No adjustable backlight detected";
    ThemeLabel => "主题", "Theme";
    ThemeDark => "深色", "Dark";
    ThemeLight => "浅色", "Light";
    SleepAfter => "自动睡眠", "Sleep After";
    Never => "永不", "Never";
    Minutes => "{} 分钟", "{} min";
    Volume => "音量", "Volume";
    NoAudio => "未检测到音频设备", "No audio device detected";
    NoAudioBody => "这台设备的系统还没有声卡驱动。", "This device's system has no sound driver yet.";

    BatteryLevel => "电量", "Charge";
    BatteryState => "状态", "Status";
    Charging => "正在充电", "Charging";
    Discharging => "使用电池", "On battery";
    Full => "已充满", "Fully charged";
    PowerDraw => "功耗", "Power Draw";
    ChargePower => "充电功率", "Charging Power";
    TimeLeft => "预计可用", "Time Left";
    HoursMinutes => "{} 小时 {} 分钟", "{} h {} min";
    Performance => "性能", "Performance";
    CpuClock => "处理器", "CPU";
    GpuClock => "图形处理器", "GPU";
    CpuBusy => "{} MHz · 占用 {}%", "{} MHz · {}% busy";
    NoBattery => "未检测到电池", "No battery detected";
    NoBatteryBody => "这台设备的系统还没有电池驱动，暂时无法显示电量。", "This device's system has no battery driver yet, so the charge can't be shown.";
    BatteryPercent => "在状态栏显示电量百分比", "Show Percentage in the Status Bar";

    ButtonTestOpen => "打开按键测试", "Open Button Test";
    SwapAb => "交换 A/B 键", "Swap A/B";
    SwapAbBody => "用下方按键确认，右侧按键返回", "Confirm with the bottom button, go back with the right one";
    Gamepads => "已连接的控制器", "Connected Controllers";
    NoGamepads => "未检测到手柄，可使用键盘", "No gamepad detected. The keyboard works too";

    LangAuto => "跟随系统", "System Default";

    Clock24 => "24 小时制", "24-Hour Clock";
    TimeNow => "当前时间", "Current Time";
    NetworkTime => "网络对时", "Network Time";
    TimeSynced => "已同步", "Synchronized";
    TimeNotSynced => "未同步，联网后自动校准", "Not yet, sets itself once online";
    TimeZone => "时区", "Time Zone";
    About => "关于本机", "About";
    Version => "Launcher 版本", "Launcher Version";
    Device => "设备", "Device";
    OperatingSystem => "操作系统", "Operating System";
    Kernel => "内核", "Kernel";
    Arch => "架构", "Architecture";
    Memory => "内存", "Memory";
    Cpus => "处理器核心", "CPU Cores";
    Hostname => "主机名", "Hostname";
    HostnameTitle => "主机名（字母、数字和连字符，留空恢复默认）", "Hostname (letters, digits and hyphens; empty for the default)";
    HostnameInvalid => "主机名最多 63 个字母、数字或连字符，不能以连字符开头或结尾", "Use up to 63 letters, digits and hyphens, not starting or ending with a hyphen";
    HostnameSaved => "主机名已改为 {}", "Hostname changed to {}";
    HostnameDefault => "已恢复默认主机名", "Hostname reset to the default";
    LocalAddress => "局域网地址", "Local Address";
    IpAddress => "IP 地址", "IP Address";
    Storage => "存储空间", "Storage";
    StorageUsed => "已使用 {} / 共 {}", "{} of {} used";
    Simulated => "模拟设备", "Simulated Devices";
    SimulatedOn => "已开启（GIZLET_FAKE_DEVICES）", "On (GIZLET_FAKE_DEVICES)";
    Unknown => "未知", "Unknown";

    KbDone => "完成", "Done";
    KbSelectOrHold => "选择 · 长按 {} 秒完成", "Select · Hold {}s to finish";
    KbSpace => "空格", "Space";
    KbShow => "显示", "Show";
    KbHide => "隐藏", "Hide";

    TestTitle => "按键测试", "Button Test";
    TestHint => "按下任意按键或推动摇杆查看反馈", "Press any button or move a stick to see it light up";
    TestExit => "按住 {} 退出", "Hold {} to exit";
    TestNoPad => "未检测到手柄 · 键盘按键同样会显示", "No gamepad · keyboard keys are shown too";
    LeftStick => "左摇杆", "Left Stick";
    RightStick => "右摇杆", "Right Stick";
    VolumeKeys => "音量键", "Volume Keys";

    Failed => "操作失败：{}", "Failed: {}";
}

impl T {
    /// The string with each `{}` replaced by the next argument.
    pub fn fmt(self, lang: Language, args: &[&str]) -> String {
        let mut out = String::new();
        let mut args = args.iter();
        let mut parts = self.get(lang).split("{}").peekable();
        while let Some(part) = parts.next() {
            out.push_str(part);
            if parts.peek().is_some() {
                out.push_str(args.next().copied().unwrap_or(""));
            }
        }
        out
    }
}

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut unit = 0;
    while v >= 1024.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    if unit == 0 || v >= 100.0 {
        format!("{:.0} {}", v, UNITS[unit])
    } else {
        format!("{:.1} {}", v, UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_and_sizes() {
        assert_eq!(T::WifiPassphrase.fmt(Language::En, &["Home"]), "Password for \"Home\"");
        assert_eq!(T::StorageUsed.fmt(Language::ZhCn, &["1 GB", "8 GB"]), "已使用 1 GB / 共 8 GB");
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(human_size(300 << 20), "300 MB");
    }
}
