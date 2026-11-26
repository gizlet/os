# Writes Gizlet OS to a TF card for the Game Console R36S, on Windows.
#
# In PowerShell opened as Administrator:
#   irm https://raw.githubusercontent.com/gizlet/os/main/boards/gameconsole/r36s/flash.ps1 | iex
#
# It asks for a language and the card's disk, and for the handheld's panel
# unless the card tells: a stock card's kernel dtb names the panel's
# controller, and a Gizlet OS card keeps the overlay it was written with. It
# downloads the matching image from the latest release and checks its SHA-256,
# then writes the card. A card that already has Gizlet OS can be updated: only
# its boot and system partitions are written, and the data partition, with the
# installed programs, saves, Wi-Fi networks and settings, is kept. Otherwise
# everything on the card is erased, which takes three confirmations: the disk
# number, a yes, and the disk's name typed out.
#
# Optional environment variables:
#   GIZLET_LANG      en or zh, to skip the language question
#   GIZLET_PANEL     v30-panel4 or original, to skip the panel question
#   GIZLET_IMAGE     a local .img or .img.gz to write instead of downloading
#   GIZLET_MODE      update or erase, to skip the question for a Gizlet OS card
#   GIZLET_RELEASE   another release download URL

# Extends partition 3 of the image's MBR in $mbr, the data partition and the
# last one, to the end of a disk of $diskBytes (aligned down to 1 MiB; MBR
# sizes stop at 2 TiB). Grow-DataFilesystem then grows the filesystem inside it.
function Grow-DataPartition([byte[]]$mbr, [long]$diskBytes) {
    $entry = 478 # partition 3 of the four at byte 446
    $start = [long][BitConverter]::ToUInt32($mbr, $entry + 8)
    $sectors = [long][math]::Floor($diskBytes / 512 / 2048) * 2048 - $start
    if ($start + $sectors -gt 4294967295) { $sectors = 4294967295 - $start }
    if ($mbr[$entry + 4] -ne 0x83 -or $start -le 0 -or $sectors -le 0) {
        throw 'Unexpected partition table in the image. / 镜像的分区表不符合预期。'
    }
    [Array]::Copy([BitConverter]::GetBytes([uint32]$sectors), 0, $mbr, $entry + 12, 4)
}

# Grows the image's ext4 data filesystem, offline, to fill partition 3 of the
# grown $mbr, as the 4 KiB blocks to write over the card after the image: a
# list of card byte offsets and blocks, primary superblock last, so a write cut
# short leaves the image's filesystem as it was. The same as grown_data in
# flash.sh, which explains the layout; an image without it (an older release)
# gives no blocks, and the card's first boot grows its filesystem instead.
function Grow-DataFilesystem([byte[]]$mbr, [string]$raw) {
    if (-not ('GizletDataGrow' -as [type])) {
        # C# 5, for Windows PowerShell 5.1's compiler.
        Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
public static class GizletDataGrow {
    const long Bpg = 32768;
    static uint[] table;
    static uint Crc(uint c, byte[] b, int off, int len) {
        if (table == null) {
            uint[] t = new uint[256];
            for (uint i = 0; i < 256; i++) {
                uint x = i;
                for (int j = 0; j < 8; j++) x = (x & 1) != 0 ? (x >> 1) ^ 0x82F63B78u : x >> 1;
                t[i] = x;
            }
            table = t;
        }
        for (int i = 0; i < len; i++) c = table[(c ^ b[off + i]) & 255] ^ (c >> 8);
        return c;
    }
    static long Get(byte[] b, int off, int size) {
        long r = 0;
        for (int i = size - 1; i >= 0; i--) r = (r << 8) | b[off + i];
        return r;
    }
    static void Put(byte[] b, int off, int size, long v) {
        for (int i = 0; i < size; i++) b[off + i] = (byte)(v >> (8 * i));
    }
    // Groups 0 and 1 and the powers of 3, 5 and 7 below the group count, in
    // the order grown_data takes them.
    static List<long> Supers(long groups) {
        List<long> s = new List<long>();
        s.Add(0); s.Add(1);
        foreach (long p in new long[] { 3, 5, 7 })
            for (long n = p; n < groups; n *= p) s.Add(n);
        return s;
    }
    static long Overhead(long g, long meta, long itb, List<long> supers, out long hs, out long gd) {
        hs = supers.Contains(g) ? 1 : 0;
        if (g < meta * 64) gd = hs * meta;
        else { long m = g % 64; gd = (m == 0 || m == 1 || m == 63) ? 1 : 0; }
        return hs + gd + 2 + itb;
    }
    static void Add(List<KeyValuePair<long, byte[]>> list, long cardBlock, byte[] from, int index) {
        byte[] block = new byte[4096];
        Array.Copy(from, index * 4096, block, 0, 4096);
        list.Add(new KeyValuePair<long, byte[]>(cardBlock * 4096, block));
    }
    // head: the image's first 8 KiB of the data partition, at startSector.
    public static List<KeyValuePair<long, byte[]>> Blocks(byte[] head, long startSector, long sectors) {
        List<KeyValuePair<long, byte[]>> list = new List<KeyValuePair<long, byte[]>>();
        if (startSector % 8 != 0 || head.Length < 8192) return list;
        long bas = startSector / 8;
        byte[] sb = new byte[1024];
        Array.Copy(head, 1024, sb, 0, 1024);
        long incompat = Get(sb, 0x60, 4), ro = Get(sb, 0x64, 4);
        if (Get(sb, 0x38, 2) != 0xEF53 || Get(sb, 0x18, 4) != 2 || Get(sb, 0x14, 4) != 0) return list;
        if (Get(sb, 0x20, 4) != Bpg || Get(sb, 0x24, 4) != Bpg || Get(sb, 0x04, 4) != Bpg || Get(sb, 0x150, 4) != 0) return list;
        if ((Get(sb, 0x5C, 4) & 0x210) != 0) return list;
        if ((incompat & 0x290) != 0x290 || (incompat & ~0x22D2L) != 0) return list;
        if ((ro & 0x401) != 0x401 || (ro & 0x210) != 0) return list;
        if (Get(sb, 0xFE, 2) != 64 || Get(sb, 0xCE, 2) != 0 || Get(sb, 0x3A, 2) != 1) return list;
        long meta = Get(sb, 0x104, 4);
        if (meta > 1) return list;
        long ipg = Get(sb, 0x28, 4), isz = Get(sb, 0x58, 2);
        if (ipg <= 0 || ipg >= 65536 || ipg * isz % 4096 != 0) return list;
        long itb = ipg * isz / 4096;
        uint seed = (incompat & 0x2000) != 0 ? (uint)Get(sb, 0x270, 4) : Crc(0xFFFFFFFFu, sb, 0x68, 16);
        long part = sectors / 8;
        long groups = (part + Bpg - 1) / Bpg;
        if (groups <= 1) return list;
        List<long> supers = Supers(groups);
        long hs, gd;
        long last = part - (groups - 1) * Bpg;
        if (last < Overhead(groups - 1, meta, itb, supers, out hs, out gd) + 50) { groups--; last = Bpg; }
        if (groups <= 1) return list;
        long blocks = (groups - 1) * Bpg + last;
        long descBlocks = (groups + 63) / 64;
        byte[] gdt = new byte[descBlocks * 4096];
        Array.Copy(head, 4096, gdt, 0, 64);
        byte[] bitmap = new byte[4096];
        long freeAdd = 0, overAdd = 0;
        for (long g = 1; g < groups; g++) {
            long ov = Overhead(g, meta, itb, supers, out hs, out gd);
            long bb = g * Bpg + hs + gd, free, flags;
            uint csum = 0;
            if (g == groups - 1) {
                free = last - ov; flags = 1;
                for (int i = 0; i < 4096; i++) {
                    long lo = i * 8, v;
                    if (lo + 8 <= ov || lo >= last) v = 255;
                    else if (lo >= ov && lo + 8 <= last) v = 0;
                    else {
                        v = 0;
                        if (lo + 8 > last) v = 255 & ~((1L << (int)(last - lo)) - 1);
                        if (lo < ov) v |= (1L << (int)(ov - lo)) - 1;
                    }
                    bitmap[i] = (byte)v;
                }
                csum = Crc(seed, bitmap, 0, 4096);
            } else {
                free = Bpg - ov; flags = 3;
            }
            freeAdd += free; overAdd += ov;
            int at = (int)(g * 64);
            Put(gdt, at, 4, bb); Put(gdt, at + 4, 4, bb + 1); Put(gdt, at + 8, 4, bb + 2);
            Put(gdt, at + 12, 2, free); Put(gdt, at + 14, 2, ipg); Put(gdt, at + 18, 2, flags);
            Put(gdt, at + 24, 2, csum & 0xFFFF); Put(gdt, at + 28, 2, ipg); Put(gdt, at + 56, 2, csum >> 16);
            byte[] number = new byte[4];
            Put(number, 0, 4, g);
            Put(gdt, at + 30, 2, Crc(Crc(seed, number, 0, 4), gdt, at, 64) & 0xFFFF);
        }
        for (long k = 0; k < descBlocks; k++) {
            if (k < meta) {
                foreach (long g in supers)
                    if (g < groups && g < meta * 64) Add(list, bas + g * Bpg + 1 + k, gdt, (int)k);
            } else {
                foreach (long g in new long[] { k * 64, k * 64 + 1, k * 64 + 63 }) {
                    if (g >= groups) continue;
                    Overhead(g, meta, itb, supers, out hs, out gd);
                    Add(list, bas + g * Bpg + hs, gdt, (int)k);
                }
            }
        }
        Overhead(groups - 1, meta, itb, supers, out hs, out gd);
        Add(list, bas + (groups - 1) * Bpg + hs + gd, bitmap, 0);
        byte[] d = (byte[])sb.Clone();
        Put(d, 0x08, 4, Get(sb, 0x08, 4) * blocks / Bpg); Put(d, 0x154, 4, 0);
        Put(d, 0x04, 4, blocks); Put(d, 0x150, 4, 0);
        Put(d, 0x00, 4, groups * ipg);
        Put(d, 0x10, 4, Get(sb, 0x10, 4) + (groups - 1) * ipg);
        Put(d, 0x0C, 4, Get(sb, 0x0C, 4) + freeAdd); Put(d, 0x158, 4, 0);
        if (Get(sb, 0x248, 4) != 0) Put(d, 0x248, 4, Get(sb, 0x248, 4) + overAdd);
        List<long> order = new List<long>(supers);
        order.RemoveAt(0);
        order.Add(0);
        foreach (long g in order) {
            if (g >= groups) continue;
            Put(d, 0x5A, 2, g & 0xFFFF);
            Put(d, 0x3FC, 4, Crc(0xFFFFFFFFu, d, 0, 1020));
            byte[] block = new byte[4096];
            if (g == 0) {
                // The primary superblock sits 1 KiB into block 0.
                Array.Copy(head, 0, block, 0, 4096);
                Array.Copy(d, 0, block, 1024, 1024);
            } else {
                Array.Copy(d, 0, block, 0, 1024);
            }
            Add(list, bas + g * Bpg, block, 0);
        }
        return list;
    }
}
'@
    }
    $start = [long][BitConverter]::ToUInt32($mbr, 478 + 8)
    $sectors = [long][BitConverter]::ToUInt32($mbr, 478 + 12)
    $head = New-Object byte[] 8192
    $file = [IO.File]::OpenRead($raw)
    try {
        [void]$file.Seek($start * 512, [IO.SeekOrigin]::Begin)
        $filled = 0
        while ($filled -lt $head.Length) {
            $n = $file.Read($head, $filled, $head.Length - $filled)
            if ($n -le 0) { break }
            $filled += $n
        }
    } finally { $file.Dispose() }
    , [GizletDataGrow]::Blocks($head, $start, $sectors)
}

# For an update: the image's MBR with the card's own partition 3 entry, which
# the card writers or its first boot extended to the end of the card, or
# $null unless the card is laid out as the image is. Partitions 1 and 2 must
# match the image's, partition 3 start where the image's does and hold the
# image's data filesystem (its ext4 UUID), of any size, and no partition 4
# may exist. The
# CHS fields are not compared: a first boot's sfdisk rewrites them.
# $imageSuper and $cardSuper are the 1 KiB ext4 superblocks of partition 3.
function Get-UpdatedMbr([byte[]]$imageMbr, [byte[]]$cardMbr, [byte[]]$imageSuper, [byte[]]$cardSuper) {
    if ($cardMbr[510] -ne 0x55 -or $cardMbr[511] -ne 0xAA) { return $null }
    foreach ($entry in 446, 462) {
        foreach ($at in @(4) + (8..15)) { if ($cardMbr[$entry + $at] -ne $imageMbr[$entry + $at]) { return $null } }
    }
    foreach ($at in @(482) + (486..489)) { if ($cardMbr[$at] -ne $imageMbr[$at]) { return $null } }
    foreach ($at in 494..509) { if ($cardMbr[$at] -ne 0) { return $null } }
    foreach ($at in @(0x38, 0x39) + (0x68..0x77)) { if ($cardSuper[$at] -ne $imageSuper[$at]) { return $null } }
    if ($imageSuper[0x38] -ne 0x53 -or $imageSuper[0x39] -ne 0xEF) { return $null }
    $updated = [byte[]]$imageMbr.Clone()
    [Array]::Copy($cardMbr, 478, $updated, 478, 16)
    , $updated
}

# The drive letter of partition 1, the FAT boot partition, of disk $number,
# given one if it has none, as 'E:'; $null when it cannot be had.
function Get-BootDrive([int]$number) {
    for ($try = 0; $try -lt 10; $try++) {
        $part = Get-Partition -DiskNumber $number -PartitionNumber 1 -ErrorAction SilentlyContinue
        if ($part -and "$($part.DriveLetter)" -match '^[A-Z]$') { return "$($part.DriveLetter):" }
        if ($part) { try { $part | Add-PartitionAccessPath -AssignDriveLetter -ErrorAction Stop } catch { } }
        Start-Sleep -Milliseconds 500
    }
    $null
}

# v30-panel4 or original for the kernel dtb at $path, by the first command of
# its DSI panel's panel-init-sequence, in Rockchip's format (type, delay,
# length, payload) right after the property's 12-byte header: NV3051D's page
# select FF 30, FF 52, FF 01 is Panel 4, and ST7703's SETEXTC B9 F1 12 83 is
# Panel 0. $null for another panel, or for the R36S Plus, whose ST7703
# sequence starts with a sleep-out. As panel_of_dtb in flash.sh.
function Get-DtbPanel([string]$path) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { return $null }
    $hex = [BitConverter]::ToString([IO.File]::ReadAllBytes($path))
    $nv = $hex -match '00-00-00-03(-[0-9A-F]{2}){8}-15-00-02-FF-30-15-00-02-FF-52-15-00-02-FF-01'
    $st = $hex -match '00-00-00-03(-[0-9A-F]{2}){8}-39-00-04-B9-F1-12-83'
    if ($nv -and -not $st) { 'v30-panel4' } elseif ($st -and -not $nv) { 'original' } else { $null }
}

# The panel the card in disk $number was made for, as @(panel, 'stock') or
# @(panel, 'gizlet'); $null when the card does not tell. A stock card's
# boot.ini loads the kernel dtb that describes the unit. A Gizlet OS card has
# mipi-panel.dtbo, a copy of one of the r36s-*.dtbo beside it, when it was
# written for a panel other than Panel 0. As card_panel in flash.sh.
function Get-CardPanel([int]$number, [bool]$gizlet) {
    try {
        $volume = Get-Partition -DiskNumber $number -PartitionNumber 1 -ErrorAction Stop | Get-Volume -ErrorAction Stop
        if ($volume.FileSystem -notmatch 'FAT') { return $null }
        $drive = Get-BootDrive $number
        if (-not $drive) { return $null }
        if ($gizlet) {
            $mipi = "$drive\mipi-panel.dtbo"
            if (-not (Test-Path -LiteralPath $mipi -PathType Leaf)) { return , @('original', 'gizlet') }
            $bytes = [IO.File]::ReadAllBytes($mipi)
            foreach ($file in @(Get-ChildItem -LiteralPath "$drive\" -Filter 'r36s-*.dtbo' -File)) {
                $other = [IO.File]::ReadAllBytes($file.FullName)
                if ($other.Length -eq $bytes.Length -and [BitConverter]::ToString($other) -eq [BitConverter]::ToString($bytes)) {
                    $panel = $file.Name -replace '^r36s-', '' -replace '\.dtbo$', ''
                    if ($panel -eq 'v30-panel4') { return , @($panel, 'gizlet') }
                }
            }
            return $null
        }
        $ini = "$drive\boot.ini"
        if (-not (Test-Path -LiteralPath $ini -PathType Leaf)) { return $null }
        # Every dtb it loads at ${dtb_loadaddr} must name the same panel.
        $found = $null
        foreach ($line in Get-Content -LiteralPath $ini) {
            $f = -split $line
            if ($f.Count -lt 5 -or $f[0] -notin 'load', 'fatload' -or $f[3] -notmatch 'dtb_loadaddr') { continue }
            if ($f[4] -notmatch '^[A-Za-z0-9_-][A-Za-z0-9._-]*$') { return $null }
            $panel = Get-DtbPanel "$drive\$($f[4])"
            if (-not $panel -or ($found -and $found -ne $panel)) { return $null }
            $found = $panel
        }
        if ($found) { return , @($found, 'stock') }
    } catch { }
    $null
}

# Files of the user's own on the boot partition that an update keeps: the ssh
# public keys, and a setup file not yet imported.
$GizletBootFiles = 'authorized_keys', 'setup.ini'

# Updates Gizlet OS on disk $disk from the image $raw: writes everything before
# the data partition, the boot code, U-Boot and the boot and system
# partitions, and keeps the data partition and the card's own partition 3
# entry. The partition table goes last, so a write cut short leaves the old
# one, and running this again finishes the update.
function Update-Card([string]$raw, $disk) {
    $number = $disk.Number
    $diskName = "Disk $number"
    $imageMbr = New-Object byte[] 512
    $imageSuper = New-Object byte[] 1024
    $cardMbr = New-Object byte[] 512
    $cardSuper = New-Object byte[] 1024
    $source = [IO.File]::OpenRead($raw)
    $target = $null
    $lock = $null
    $offline = $false
    $kept = @()
    $keep = Join-Path ([IO.Path]::GetTempPath()) ('gizlet-boot-files-' + [guid]::NewGuid())
    try {
        [void]$source.Read($imageMbr, 0, 512)
        $start = [long][BitConverter]::ToUInt32($imageMbr, 486)
        $head = $start * 512
        if ($head -le 0 -or $head % 1MB -ne 0 -or $head + 2048 -gt $source.Length) {
            throw (M 'Unexpected partition table in the image.' '镜像的分区表不符合预期。')
        }
        [void]$source.Seek($head + 1024, [IO.SeekOrigin]::Begin)
        [void]$source.Read($imageSuper, 0, 1024)
        $probe = [IO.FileStream]::new([GizletRawDisk]::Open("\\.\PhysicalDrive$number"), [IO.FileAccess]::ReadWrite)
        try {
            [void]$probe.Read($cardMbr, 0, 512)
            [void]$probe.Seek($head + 1024, [IO.SeekOrigin]::Begin)
            [void]$probe.Read($cardSuper, 0, 1024)
        } finally { $probe.Dispose() }
        $mbr = Get-UpdatedMbr $imageMbr $cardMbr $imageSuper $cardSuper
        if (-not $mbr) {
            throw (M "$diskName does not hold Gizlet OS laid out as this image, so it cannot be updated; nothing was written. Run this again and choose to erase it." "$diskName 上的 Gizlet OS 与这个镜像的分区布局不一致，无法只更新系统；没有写入任何内容。请重新运行并选择清空整张卡。")
        }
        # The user's own files on the boot partition, put back after the write.
        $drive = Get-BootDrive $number
        if ($drive) {
            foreach ($file in $GizletBootFiles) {
                if (Test-Path -LiteralPath "$drive\$file" -PathType Leaf) {
                    if (-not $kept) { New-Item -ItemType Directory $keep | Out-Null }
                    Copy-Item -LiteralPath "$drive\$file" -Destination (Join-Path $keep $file)
                    $kept += $file
                }
            }
        } else {
            # The update rewrites the boot partition, so its files must be kept first.
            throw (M "Cannot read the boot partition of $diskName to keep its authorized_keys and setup.ini; nothing was written." "无法读取 $diskName 的启动分区，不能保留上面的 authorized_keys 和 setup.ini；没有写入任何内容。")
        }
        # Windows blocks writes under a mounted volume: the disk goes offline
        # where it can, or the boot partition's volume is locked and dismounted.
        Set-Disk -Number $number -IsReadOnly $false
        try { Set-Disk -Number $number -IsOffline $true; $offline = $true } catch { }
        if (-not $offline -and $drive) { $lock = [GizletRawDisk]::LockVolume($drive) }
        $target = [IO.FileStream]::new([GizletRawDisk]::Open("\\.\PhysicalDrive$number"), [IO.FileAccess]::ReadWrite)
        $buffer = New-Object byte[] (4MB)
        [void]$source.Seek(0, [IO.SeekOrigin]::Begin)
        $written = 0L
        Write-Host (M "Updating $diskName ..." "正在更新 $diskName……")
        while ($written -lt $head) {
            $want = [int][math]::Min($buffer.Length, $head - $written)
            $filled = 0
            while ($filled -lt $want) {
                $n = $source.Read($buffer, $filled, $want - $filled)
                if ($n -le 0) { throw (M 'The image is cut short.' '镜像文件不完整。') }
                $filled += $n
            }
            # Sector 0, the partition table, is written last.
            $skip = if ($written -eq 0) { 512 } else { 0 }
            [void]$target.Seek($written + $skip, [IO.SeekOrigin]::Begin)
            $target.Write($buffer, $skip, $filled - $skip)
            $written += $filled
            Write-Host -NoNewline ("`r{0} / {1} MB" -f [int]($written / 1MB), [int]($head / 1MB))
        }
        [void]$target.Seek(0, [IO.SeekOrigin]::Begin)
        $target.Write($mbr, 0, 512)
        $target.Flush()
        Write-Host ''
    } catch {
        # $keep is outside the work directory, which is removed, so the copies stay.
        if ($kept) {
            Write-Host ''
            Write-Host ((M 'The boot partition''s own files (authorized_keys, setup.ini) are kept in' '启动分区上原有的文件（authorized_keys、setup.ini）保存在') + " $keep" + (M '; copy them back after writing the card again.' '，重新写卡后请自行复制回去。'))
        }
        throw
    } finally {
        if ($target) { $target.Dispose() }
        if ($lock) { $lock.Dispose() }
        $source.Dispose()
        if ($offline) { Set-Disk -Number $number -IsOffline $false }
    }
    Update-Disk -Number $number
    if ($kept) {
        $drive = Get-BootDrive $number
        if ($drive) {
            foreach ($file in $kept) { Copy-Item -LiteralPath (Join-Path $keep $file) -Destination "$drive\$file" -Force }
            Remove-Item -Recurse -Force $keep -ErrorAction SilentlyContinue
        } else {
            Write-Host ((M 'Warning: cannot put the boot partition''s own files back; copy them from' '警告：无法把启动分区上原有的文件放回去，请自行从这里复制回去：') + " $keep")
        }
    }
}

& {
    $ErrorActionPreference = 'Stop'
    $ProgressPreference = 'SilentlyContinue'
    # Windows PowerShell 5.1 on older Windows 10 does not offer TLS 1.2 by default; GitHub needs it.
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
    if (-not ($env:GIZLET_IMAGE -or $env:GIZLET_IMAGE_URL -or $env:GIZLET_RELEASE)) {
        $entryPath = Join-Path ([IO.Path]::GetTempPath()) "gizlet-entry-$([guid]::NewGuid()).ps1"
        try {
            Invoke-WebRequest -UseBasicParsing -Uri 'https://dl.gizlet.io/releases/latest/flash.ps1' -OutFile $entryPath
            & ([scriptblock]::Create([IO.File]::ReadAllText($entryPath))) -Board gameconsole.r36s
        } finally {
            Remove-Item -LiteralPath $entryPath -Force -ErrorAction SilentlyContinue
        }
        return
    }
    $release = if ($env:GIZLET_RELEASE) { $env:GIZLET_RELEASE } else { 'https://github.com/gizlet/os/releases/latest/download' }
    $managedImage = $env:GIZLET_IMAGE_URL
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

    # --- disks: USB or card readers, never the boot or system disk
    # Smallest first: the TF card is usually the smallest disk, and number 1 is the default.
    $disks = @(Get-Disk | Where-Object { -not $_.IsBoot -and -not $_.IsSystem -and $_.BusType -in 'USB', 'SD', 'MMC' } |
        Sort-Object Size, Number)
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
    $choice = Read-Host (M 'Which disk is the R36S card? Number [1]' '哪个是 R36S 的卡？输入编号 [1]')
    if (-not $choice) { $choice = '1' }
    if ($choice -notmatch '^\d+$' -or [int]$choice -lt 1 -or [int]$choice -gt $disks.Count) { throw (M 'No such disk.' '没有这个磁盘。') }
    $disk = $disks[[int]$choice - 1]
    $label = $labels[[int]$choice - 1]
    $diskName = "Disk $($disk.Number)"

    # --- update or erase: a card with Gizlet OS can keep its data partition
    $mode = $env:GIZLET_MODE
    if (-not $mode) {
        $mode = 'erase'
        if ($label -match 'GIZLETBOOT') {
            Write-Host ''
            Write-Host (M 'This card already has Gizlet OS. What should be written?' '这张卡上已经有 Gizlet OS。要怎么写？')
            Write-Host (M '  1) Update the system only, keeping installed programs, saves, Wi-Fi networks and settings' '  1) 只更新系统，保留已安装的程序、存档、Wi-Fi 网络和设置')
            Write-Host (M '  2) Erase the whole card and start again' '  2) 清空整张卡，重新开始')
            switch (Read-Host '[1]') {
                { $_ -in '', '1' } { $mode = 'update' }
                '2' { $mode = 'erase' }
                default { throw (M 'Answer 1 or 2.' '请输入 1 或 2。') }
            }
        }
    }
    if ($mode -notin 'update', 'erase') { throw 'GIZLET_MODE must be update or erase.' }

    # --- panel: from GIZLET_PANEL, then from the card, then from the user. A
    # stock card tells for certain, so the question is skipped. A Gizlet OS card
    # only tells what it was written with, which may be the wrong panel being
    # fixed, so it becomes the question's default.
    function PanelLabel([string]$p) {
        switch ($p) {
            'v30-panel4' { M 'Panel 4 (NV3051D)' 'Panel 4（NV3051D）' }
            'original' { M 'Panel 0 (ST7703)' 'Panel 0（ST7703）' }
        }
    }
    $panel = $env:GIZLET_PANEL
    $found = $null
    if (-not $panel) {
        $found = Get-CardPanel $disk.Number ($label -match 'GIZLETBOOT')
        if ($found -and $found[1] -eq 'stock') {
            $panel = $found[0]
            Write-Host ''
            Write-Host ((M "The stock card's kernel dtb is for {0}; the image for it will be written." '原厂卡的内核 dtb 对应 {0}，将写入对应的镜像。') -f (PanelLabel $panel))
            Write-Host (M 'If the screen stays black after boot, run this again and choose the other panel.' '如果开机后屏幕一直黑屏，重新运行并选择另一种屏幕。')
        }
    }
    if (-not $panel) {
        $default = if ($found -and $found[0] -eq 'original') { '2' } else { '1' }
        Write-Host ''
        Write-Host (M 'Gizlet OS for the R36S. Which screen does the handheld have?' 'R36S 的 Gizlet OS。掌机是哪种屏幕？')
        if ($found) { Write-Host ((M 'This card was written for {0}.' '这张卡上次写入的是 {0}。') -f (PanelLabel $found[0])) }
        Write-Host ('  1) ' + (PanelLabel 'v30-panel4') + (M ': V30 boards and most current units' '：V30 主板和目前大多数机器'))
        Write-Host ('  2) ' + (PanelLabel 'original') + (M ': earlier units' '：较早的机器'))
        Write-Host (M 'Writing over the stock card finds this by itself. If the screen stays black after boot, write the card again with the other choice.' '直接写原厂卡时会自动识别。如果开机后屏幕一直黑屏，换另一个选项重新写卡。')
        $answer = Read-Host ((M 'Panel [{0}]' '屏幕 [{0}]') -f $default)
        if (-not $answer) { $answer = $default }
        switch ($answer) {
            '1' { $panel = 'v30-panel4' }
            '2' { $panel = 'original' }
            default { throw (M 'Answer 1 or 2.' '请输入 1 或 2。') }
        }
    }
    $name = switch ($panel) {
        'v30-panel4' { 'gizlet-os-r36s-v30-panel4' }
        'original' { 'gizlet-os-r36s' }
        default { throw (M "Unknown panel $panel." "未知屏幕 $panel。") }
    }

    Write-Host ''
    Write-Host ((M 'Selected:' '已选择：') + " $diskName  $label")
    if ($mode -eq 'update') {
        # --- confirmation 2 of an update: yes. The card is checked again before writing.
        Write-Host (M 'Gizlet OS on this card will be updated. Its boot and system partitions are replaced; the data partition, with installed programs, saves, Wi-Fi networks and settings, is kept.' '将更新这张卡上的 Gizlet OS：替换启动分区和系统分区，保留数据分区，包括已安装的程序、存档、Wi-Fi 网络和设置。')
        if ((Read-Host (M 'Update it? [y/N]' '确定更新？[y/N]')) -notin 'y', 'Y', 'yes', 'YES') { throw (M 'Cancelled.' '已取消。') }
    } else {
        # --- confirmation 2: yes, after the warnings
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
    }

    # --- image
    $work = Join-Path ([IO.Path]::GetTempPath()) ('gizlet-' + [guid]::NewGuid())
    New-Item -ItemType Directory $work | Out-Null
    try {
        $image = $env:GIZLET_IMAGE
        if ($image -and -not $managedImage) {
            if (-not (Test-Path -LiteralPath $image -PathType Leaf)) { throw (M "No such file $image." "文件不存在：$image。") }
            # .NET resolves relative paths against the process's directory, not PowerShell's.
            $image = (Resolve-Path -LiteralPath $image).ProviderPath
        } else {
            $imageUrl = if ($managedImage) { $managedImage } else { "$release/$name.img.gz" }
            $sumsUrl = if ($managedImage) { $env:GIZLET_CHECKSUM_URL } else { "$release/SHA256SUMS" }
            $file = ([uri]$imageUrl).Segments[-1]
            $image = Join-Path $work $file
            Write-Host ((M 'Downloading' '正在下载') + " $file ...")
            $downloadOptions = if ($managedImage) { @{ MaximumRedirection = 0 } } else { @{} }
            Invoke-WebRequest -UseBasicParsing -Uri $imageUrl -OutFile $image @downloadOptions
            $sums = (Invoke-WebRequest -UseBasicParsing -Uri $sumsUrl @downloadOptions).Content
            if ($sums -is [byte[]]) { $sums = [Text.Encoding]::ASCII.GetString($sums) }
            $line = $sums -split "`n" | Where-Object { $_ -match "^([0-9a-f]{64}) +\*?$([regex]::Escape($file))\s*$" } | Select-Object -First 1
            if (-not $line) { throw (M "SHA256SUMS does not list $file." "SHA256SUMS 里没有 $file。") }
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
        $imageBytes = (Get-Item -LiteralPath $raw).Length
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
    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool DeviceIoControl(SafeFileHandle device, uint code, IntPtr input, uint inputSize,
        IntPtr output, uint outputSize, out uint returned, IntPtr overlapped);
    // Locks and dismounts a volume such as "E:", so the disk's sectors under it
    // can be written while the handle stays open.
    public static SafeFileHandle LockVolume(string drive) {
        SafeFileHandle handle = Open("\\\\.\\" + drive);
        uint returned;
        // FSCTL_LOCK_VOLUME, FSCTL_DISMOUNT_VOLUME
        if (!DeviceIoControl(handle, 0x90018, IntPtr.Zero, 0, IntPtr.Zero, 0, out returned, IntPtr.Zero) ||
            !DeviceIoControl(handle, 0x90020, IntPtr.Zero, 0, IntPtr.Zero, 0, out returned, IntPtr.Zero)) {
            int error = Marshal.GetLastWin32Error();
            handle.Dispose();
            throw new System.ComponentModel.Win32Exception(error);
        }
        return handle;
    }
}
'@
        }

        if ($mode -eq 'update') {
            Update-Card $raw $disk
        } else {
            # The image is open before anything on the card changes, so a file that
            # cannot be read leaves the card as it was.
            $mbr = New-Object byte[] 512
            $source = [IO.File]::OpenRead($raw)
            [void]$source.Read($mbr, 0, 512)
            [void]$source.Seek(0, [IO.SeekOrigin]::Begin)
            Grow-DataPartition $mbr $disk.Size
            $grow = Grow-DataFilesystem $mbr $raw
            $target = $null
            $offline = $false
            try {
                # --- write. Windows blocks writes over a mounted volume, and often will not take a
                # removable disk offline, so the partition table is cleared first: then no volume
                # holds the disk. Offline is tried as well where it is supported.
                Set-Disk -Number $disk.Number -IsReadOnly $false
                if ($disk.PartitionStyle -ne 'RAW') { Clear-Disk -Number $disk.Number -RemoveData -RemoveOEM -Confirm:$false }
                try { Set-Disk -Number $disk.Number -IsOffline $true; $offline = $true } catch { }
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
                    if ($written -eq 0) { [Array]::Copy($mbr, $buffer, 512) }
                    $length = [int]([math]::Ceiling($filled / 512) * 512)
                    if ($length -gt $filled) { [Array]::Clear($buffer, $filled, $length - $filled) }
                    $target.Write($buffer, 0, $length)
                    $written += $filled
                    Write-Host -NoNewline ("`r{0} / {1} MB" -f [int]($written / 1MB), [int]($imageBytes / 1MB))
                    if ($filled -lt $buffer.Length) { break }
                }
                foreach ($block in $grow) {
                    [void]$target.Seek($block.Key, [IO.SeekOrigin]::Begin)
                    $target.Write($block.Value, 0, $block.Value.Length)
                }
                $target.Flush()
                Write-Host ''
            } finally {
                if ($target) { $target.Dispose() }
                $source.Dispose()
                if ($offline) { Set-Disk -Number $disk.Number -IsOffline $false }
            }
            Update-Disk -Number $disk.Number
        }
        if ($managedImage -and $panel -eq 'v30-panel4') {
            try {
                $drive = Get-BootDrive $disk.Number
                if (-not $drive) { throw 'Boot partition unavailable.' }
                Copy-Item -LiteralPath "$drive\r36s-v30-panel4.dtbo" -Destination "$drive\mipi-panel.dtbo" -Force
            } catch {
                throw (M 'Card written, but Panel 4 setup failed. Copy r36s-v30-panel4.dtbo to mipi-panel.dtbo on GIZLETBOOT.' '已写卡，但无法配置 Panel 4。请在 GIZLETBOOT 上把 r36s-v30-panel4.dtbo 复制为 mipi-panel.dtbo。')
            }
        }
    } finally {
        Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
    }
    Write-Host ''
    if ($mode -eq 'update') {
        Write-Host (M 'Done: Gizlet OS is updated and the data partition kept. Eject the card, put it in the R36S and switch it on.' '完成：Gizlet OS 已更新，数据分区保留。弹出卡，插进 R36S 开机。')
    } else {
        Write-Host (M 'Done. Eject the card, put it in the R36S and switch it on.' '完成。弹出卡，插进 R36S 开机。')
    }
}
