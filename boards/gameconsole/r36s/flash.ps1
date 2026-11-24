# Writes Gizlet OS to a TF card for the Game Console R36S, on Windows.
#
# In PowerShell opened as Administrator:
#   irm https://raw.githubusercontent.com/gizlet/os/main/boards/gameconsole/r36s/flash.ps1 | iex
#
# It asks for a language, the handheld's panel and the card's disk, downloads
# the matching image from the latest release and checks its SHA-256, then
# writes the whole card. Everything on the card is erased, so erasing takes
# three confirmations: the disk number, a yes, and the disk's name typed out.
#
# Optional environment variables:
#   GIZLET_LANG      en or zh, to skip the language question
#   GIZLET_PANEL     v30-panel4 or original, to skip the panel question
#   GIZLET_IMAGE     a local .img or .img.gz to write instead of downloading
#   GIZLET_RELEASE   another release download URL

& {
    $ErrorActionPreference = 'Stop'
    $ProgressPreference = 'SilentlyContinue'
    # Windows PowerShell 5.1 on older Windows 10 does not offer TLS 1.2 by default; GitHub needs it.
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
    $release = if ($env:GIZLET_RELEASE) { $env:GIZLET_RELEASE } else { 'https://github.com/gizlet/os/releases/latest/download' }
    $large = 256GB

    # --- language
    $lang = $env:GIZLET_LANG
    if (-not $lang) {
        Write-Host 'Language / 语言:'
        Write-Host '  1) English'
        Write-Host '  2) 中文'
        switch (Read-Host '[1]') {
            { $_ -in '', '1' } { $lang = 'en' }
            '2' { $lang = 'zh' }
            default { throw 'Answer 1 or 2 / 请输入 1 或 2' }
        }
    }
    if ($lang -notin 'en', 'zh') { throw 'GIZLET_LANG must be en or zh.' }
    function M([string]$en, [string]$zh) { if ($lang -eq 'zh') { $zh } else { $en } }

    $admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
        [Security.Principal.WindowsBuiltInRole]::Administrator)
    if (-not $admin) { throw (M 'Open PowerShell as Administrator and run this again.' '请以管理员身份打开 PowerShell 后重新运行。') }

    # --- panel
    $panel = $env:GIZLET_PANEL
    if (-not $panel) {
        Write-Host ''
        Write-Host (M 'Gizlet OS for the R36S. Which screen does the handheld have?' 'R36S 的 Gizlet OS。掌机是哪种屏幕？')
        Write-Host (M '  1) V30 board with Panel 4 (most current units)' '  1) V30 主板 + Panel 4（目前大多数机器）')
        Write-Host (M '  2) Original panel (ST7703, Panel 0)' '  2) 原版屏幕（ST7703，Panel 0）')
        Write-Host (M 'If the screen stays black after boot, write the card again with the other choice.' '如果开机后屏幕一直黑屏，换另一个选项重新写卡。')
        switch (Read-Host (M 'Panel [1]' '屏幕 [1]')) {
            { $_ -in '', '1' } { $panel = 'v30-panel4' }
            '2' { $panel = 'original' }
            default { throw (M 'Answer 1 or 2.' '请输入 1 或 2。') }
        }
    }
    $name = switch ($panel) {
        'v30-panel4' { 'gizlet-os-r36s-v30-panel4' }
        'original' { 'gizlet-os-r36s' }
        default { throw (M "Unknown panel $panel." "未知屏幕 $panel。") }
    }

    # --- disks: USB or card readers, never the boot or system disk
    $disks = @(Get-Disk | Where-Object { -not $_.IsBoot -and -not $_.IsSystem -and $_.BusType -in 'USB', 'SD', 'MMC' } |
        Sort-Object Number)
    if ($disks.Count -eq 0) { throw (M 'No external disk found. Insert the TF card and try again.' '没有找到外接磁盘，请插入 TF 卡后重试。') }
    Write-Host ''
    Write-Host (M 'External disks:' '外接磁盘：')
    $labels = @{}
    for ($i = 0; $i -lt $disks.Count; $i++) {
        $d = $disks[$i]
        $volumes = @(Get-Partition -DiskNumber $d.Number -ErrorAction SilentlyContinue |
            Get-Volume -ErrorAction SilentlyContinue | ForEach-Object { $_.FileSystemLabel } | Where-Object { $_ })
        $labels[$i] = '{0:N1} GB  {1}  [{2}]' -f ($d.Size / 1e9), $d.FriendlyName, ($volumes -join ',')
        $note = if ($d.Size -gt $large) { '  ' + (M '<- over 256 GB, probably not a TF card' '<- 超过 256 GB，可能不是 TF 卡') } else { '' }
        Write-Host ('  {0}) Disk {1}  {2}{3}' -f ($i + 1), $d.Number, $labels[$i], $note)
    }

    # --- confirmation 1: the disk number
    $choice = Read-Host (M 'Which disk is the R36S card? Number' '哪个是 R36S 的卡？输入编号')
    if ($choice -notmatch '^\d+$' -or [int]$choice -lt 1 -or [int]$choice -gt $disks.Count) { throw (M 'No such disk.' '没有这个磁盘。') }
    $disk = $disks[[int]$choice - 1]
    $label = $labels[[int]$choice - 1]
    $diskName = "Disk $($disk.Number)"

    # --- confirmation 2: yes, after the warnings
    Write-Host ''
    Write-Host ((M 'Selected:' '已选择：') + " $diskName  $label")
    if ($disk.Size -gt $large) {
        Write-Host (M 'WARNING: this disk is over 256 GB. TF cards are rarely this large; it may be an external drive.' '警告：这个磁盘超过 256 GB，TF 卡很少这么大，它可能是移动硬盘。')
    }
    if ($label -match 'ROMS') {
        Write-Host (M 'WARNING: this looks like a stock ArkOS card. Its games will be lost; keep it as a backup if you can.' '警告：这看起来是原厂 ArkOS 卡，上面的游戏会全部丢失。建议保留它作备份，换一张卡。')
    }
    Write-Host (M 'ALL DATA on this disk will be ERASED and cannot be recovered.' '这个磁盘上的所有数据都会被清除，无法恢复。')
    if ((Read-Host (M 'Erase it? [y/N]' '确定清除？[y/N]')) -notin 'y', 'Y', 'yes', 'YES') { throw (M 'Cancelled.' '已取消。') }

    # --- confirmation 3: type the disk's name, as GitHub asks for a repository's name
    Write-Host ''
    Write-Host ((M 'To confirm, type the disk name:' '请输入磁盘名称以确认：') + " $diskName")
    if ((Read-Host '>').Trim() -ne $diskName) { throw (M 'The name does not match; cancelled.' '名称不一致，已取消。') }

    # --- image
    $work = Join-Path ([IO.Path]::GetTempPath()) ('gizlet-' + [guid]::NewGuid())
    New-Item -ItemType Directory $work | Out-Null
    try {
        $image = $env:GIZLET_IMAGE
        if ($image) {
            if (-not (Test-Path $image)) { throw (M "No such file $image." "文件不存在：$image。") }
        } else {
            $image = Join-Path $work "$name.img.gz"
            Write-Host ((M 'Downloading' '正在下载') + " $name.img.gz ...")
            Invoke-WebRequest -UseBasicParsing -Uri "$release/$name.img.gz" -OutFile $image
            $sums = (Invoke-WebRequest -UseBasicParsing -Uri "$release/SHA256SUMS").Content
            if ($sums -is [byte[]]) { $sums = [Text.Encoding]::ASCII.GetString($sums) }
            $line = $sums -split "`n" | Where-Object { $_ -match "^([0-9a-f]{64}) +\*?$([regex]::Escape("$name.img.gz"))\s*$" } | Select-Object -First 1
            if (-not $line) { throw (M "SHA256SUMS does not list $name.img.gz." "SHA256SUMS 里没有 $name.img.gz。") }
            $expected = ($line -split ' ')[0]
            if ((Get-FileHash -Algorithm SHA256 $image).Hash -ne $expected.ToUpperInvariant()) {
                throw (M 'The download is corrupt (SHA-256 mismatch).' '下载的文件已损坏（SHA-256 不一致）。')
            }
        }

        # Unpack before the card is touched, so a corrupt image changes nothing.
        $raw = $image
        if ($image -like '*.gz') {
            Write-Host (M 'Unpacking the image ...' '正在解压镜像……')
            $raw = Join-Path $work 'card.img'
            $gz = [IO.Compression.GZipStream]::new([IO.File]::OpenRead($image), [IO.Compression.CompressionMode]::Decompress)
            $out = [IO.File]::Create($raw)
            try { $gz.CopyTo($out) } finally { $out.Dispose(); $gz.Dispose() }
        }
        $imageBytes = (Get-Item $raw).Length
        # The card may have been swapped while the image downloaded.
        $now = Get-Disk -Number $disk.Number -ErrorAction SilentlyContinue
        if (-not $now -or $now.Size -ne $disk.Size -or $now.UniqueId -ne $disk.UniqueId) {
            throw (M "$diskName changed since it was chosen; cancelled." "$diskName 在选择之后发生了变化，已取消。")
        }
        if ($disk.Size -lt $imageBytes) { throw (M 'The card is smaller than the image.' '这张卡比镜像还小。') }

        # Windows PowerShell's FileStream refuses \\.\ device paths; open the disk with CreateFile.
        if (-not ('GizletRawDisk' -as [type])) {
            Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
public static class GizletRawDisk {
    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    static extern SafeFileHandle CreateFile(string name, uint access, uint share, IntPtr security,
        uint disposition, uint flags, IntPtr template);
    public static SafeFileHandle Open(string name) {
        // GENERIC_READ | GENERIC_WRITE, FILE_SHARE_READ | FILE_SHARE_WRITE, OPEN_EXISTING, FILE_FLAG_WRITE_THROUGH
        SafeFileHandle handle = CreateFile(name, 0xC0000000, 3, IntPtr.Zero, 3, 0x80000000, IntPtr.Zero);
        if (handle.IsInvalid) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
        return handle;
    }
}
'@
        }

        # --- write. Windows blocks writes over a mounted volume, and often will not take a
        # removable disk offline, so the partition table is cleared first: then no volume
        # holds the disk. Offline is tried as well where it is supported.
        Set-Disk -Number $disk.Number -IsReadOnly $false
        if ($disk.PartitionStyle -ne 'RAW') { Clear-Disk -Number $disk.Number -RemoveData -RemoveOEM -Confirm:$false }
        $offline = $false
        try { Set-Disk -Number $disk.Number -IsOffline $true; $offline = $true } catch { }
        $source = [IO.File]::OpenRead($raw)
        $target = $null
        try {
            $target = [IO.FileStream]::new([GizletRawDisk]::Open("\\.\PhysicalDrive$($disk.Number)"), [IO.FileAccess]::ReadWrite)
            $buffer = New-Object byte[] (4MB)
            $written = 0L
            Write-Host (M "Writing $diskName, about a minute ..." "正在写入 $diskName，大约一分钟……")
            while ($true) {
                # Fill the whole buffer: writes to a disk must be whole sectors.
                $filled = 0
                while ($filled -lt $buffer.Length) {
                    $n = $source.Read($buffer, $filled, $buffer.Length - $filled)
                    if ($n -le 0) { break }
                    $filled += $n
                }
                if ($filled -eq 0) { break }
                $length = [int]([math]::Ceiling($filled / 512) * 512)
                if ($length -gt $filled) { [Array]::Clear($buffer, $filled, $length - $filled) }
                $target.Write($buffer, 0, $length)
                $written += $filled
                Write-Host -NoNewline ("`r{0} / {1} MB" -f [int]($written / 1MB), [int]($imageBytes / 1MB))
                if ($filled -lt $buffer.Length) { break }
            }
            $target.Flush()
            Write-Host ''
        } finally {
            if ($target) { $target.Dispose() }
            $source.Dispose()
            if ($offline) { Set-Disk -Number $disk.Number -IsOffline $false }
        }
        Update-Disk -Number $disk.Number
    } finally {
        Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
    }
    Write-Host ''
    Write-Host (M 'Done. Eject the card, put it in the R36S and switch it on.' '完成。弹出卡，插进 R36S 开机。')
    Write-Host (M 'The first boot grows the data partition to the whole card and opens the launcher.' '第一次开机会把数据分区扩展到整张卡，然后进入 launcher。')
}
