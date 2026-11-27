param(
    [string]$Release = $env:GIZLET_RELEASE,
    [ValidateSet('en', 'zh')][string]$Language = $env:GIZLET_LANG,
    [string]$Board
)

& {
    $ErrorActionPreference = 'Stop'
    $ProgressPreference = 'SilentlyContinue'
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
    if (-not $Language) {
        Write-Host 'Language / 语言:'
        Write-Host '  1) English'
        Write-Host '  2) 中文'
        switch (Read-Host '[1]') {
            { $_ -in '', '1' } { $Language = 'en' }
            '2' { $Language = 'zh' }
            default { throw 'Answer 1 or 2 / 请输入 1 或 2' }
        }
    }
    if (-not $Release) { $Release = 'https://gizlet.io/assets/releases/@RELEASE_TAG@' }
    $Release = $Release.TrimEnd('/')
    if ($Release -ceq 'https://gizlet.io/assets/releases/@RELEASE_TAG@') {
        $latest = Invoke-RestMethod -Uri 'https://gizlet.io/api/v1/os/releases/latest' -MaximumRedirection 0
        if ($latest.tag -cnotmatch '\Av(0|[1-9][0-9]{0,8})(\.(0|[1-9][0-9]{0,8})){2}\z') { throw 'Invalid Gizlet OS release tag.' }
        $Release = "https://gizlet.io/assets/releases/$($latest.tag)"
    }
    if ($Release -cnotmatch '^https://[^/]+/') { throw 'The release URL must use HTTPS.' }
    $manifest = Invoke-RestMethod -Uri "$Release/manifest.json" -MaximumRedirection 0
    if ($manifest.schema_version -ne 1) { throw 'Unsupported release manifest.' }
    $boards = @(
        foreach ($category in $manifest.boards.PSObject.Properties) {
            foreach ($entry in $category.Value.PSObject.Properties) {
                [PSCustomObject]@{ Id = "$($category.Name).$($entry.Name)"; Assets = $entry.Value }
            }
        }
    )
    if ($boards.Count -eq 0) { throw 'This release contains no boards.' }
    if (-not $Board) {
        if ($Language -eq 'zh') { Write-Host '请选择设备：' } else { Write-Host 'Select your board:' }
        for ($index = 0; $index -lt $boards.Count; $index++) {
            Write-Host "  $($index + 1)) $($boards[$index].Id)"
        }
        $answer = Read-Host '[1]'
        if (-not $answer) { $answer = '1' }
        $selected = 0
        if (-not [int]::TryParse($answer, [ref]$selected) -or $selected -lt 1 -or $selected -gt $boards.Count) {
            throw 'Invalid selection / 无效选项'
        }
        $Board = $boards[$selected - 1].Id
    }
    $entry = $boards | Where-Object Id -CEQ $Board | Select-Object -First 1
    if (-not $entry) { throw 'Invalid board / 无效设备' }
    function Get-AssetUrl([string]$Path) {
        if ($Path -cnotmatch '\A[A-Za-z0-9_-][A-Za-z0-9_.-]*(/[A-Za-z0-9_-][A-Za-z0-9_.-]*)*\z') {
            throw 'Invalid release asset path.'
        }
        "$Release/$Path"
    }
    $scriptUrl = Get-AssetUrl $entry.Assets.flash_scripts.windows
    $imageUrl = Get-AssetUrl $entry.Assets.image.path
    $checksumUrl = Get-AssetUrl $entry.Assets.image.checksum
    $savedLanguage = $env:GIZLET_LANG
    $savedImageUrl = $env:GIZLET_IMAGE_URL
    $savedChecksumUrl = $env:GIZLET_CHECKSUM_URL
    $scriptPath = Join-Path ([IO.Path]::GetTempPath()) "gizlet-flash-$([guid]::NewGuid()).ps1"
    try {
        Invoke-WebRequest -UseBasicParsing -Uri $scriptUrl -MaximumRedirection 0 -OutFile $scriptPath
        $env:GIZLET_LANG = $Language
        $env:GIZLET_IMAGE_URL = $imageUrl
        $env:GIZLET_CHECKSUM_URL = $checksumUrl
        & ([scriptblock]::Create([IO.File]::ReadAllText($scriptPath)))
    } finally {
        $env:GIZLET_LANG = $savedLanguage
        $env:GIZLET_IMAGE_URL = $savedImageUrl
        $env:GIZLET_CHECKSUM_URL = $savedChecksumUrl
        Remove-Item -LiteralPath $scriptPath -Force -ErrorAction SilentlyContinue
    }
}
