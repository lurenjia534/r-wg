param(
    [string]$TunName = "",
    [string]$Endpoint = "",
    [int]$EndpointPort = 51820,
    [string]$OutFile = ""
)

function Write-Section {
    param([string]$Title)
    Write-Output ""
    Write-Output "==== $Title ===="
}

$timestamp = Get-Date -Format "yyyyMMdd-HHmmss"
if (-not $OutFile) {
    $OutFile = "r-wg-diag-$timestamp.txt"
}

try {
    Start-Transcript -Path $OutFile -Force | Out-Null
} catch {
    Write-Output "Failed to start transcript: $($_.Exception.Message)"
}

Write-Section "Basic Info"
Write-Output ("Time: {0}" -f (Get-Date))
Write-Output ("PSVersion: {0}" -f $PSVersionTable.PSVersion)
Write-Output ("User: {0}" -f $env:USERNAME)
Write-Output ("Computer: {0}" -f $env:COMPUTERNAME)

$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).
    IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
Write-Output ("Admin: {0}" -f $isAdmin)

Write-Section "OS"
Get-CimInstance -ClassName Win32_OperatingSystem |
    Select-Object Caption, Version, BuildNumber, OSArchitecture |
    Format-List

Write-Section "Network Adapters"
Get-NetAdapter | Sort-Object Status, Name | Format-Table -AutoSize

if (-not $TunName) {
    $candidate = Get-NetAdapter | Where-Object {
        $_.Name -like "*wg*" -or $_.InterfaceDescription -like "*WireGuard*" -or $_.InterfaceDescription -like "*Wintun*"
    } | Select-Object -First 1
    if ($candidate) {
        $TunName = $candidate.Name
        Write-Output ("Detected tunnel adapter: {0}" -f $TunName)
    } else {
        Write-Output "Tunnel adapter not detected by name; set -TunName explicitly."
    }
}

Write-Section "IP Interfaces"
Get-NetIPInterface | Sort-Object InterfaceAlias, AddressFamily |
    Format-Table -AutoSize

if ($TunName) {
    Write-Section "Tunnel IP Address"
    Get-NetIPAddress -InterfaceAlias $TunName | Format-Table -AutoSize

    Write-Section "Tunnel Routes"
    Get-NetRoute -InterfaceAlias $TunName | Sort-Object DestinationPrefix | Format-Table -AutoSize
}

Write-Section "Default Routes"
Get-NetRoute -DestinationPrefix @("0.0.0.0/0", "::/0") |
    Sort-Object RouteMetric, InterfaceMetric |
    Format-Table -AutoSize

Write-Section "DNS"
Get-DnsClientServerAddress | Format-Table -AutoSize
Get-DnsClient | Select-Object InterfaceAlias, ConnectionSpecificSuffix, RegisterThisConnectionsAddress |
    Format-Table -AutoSize

Write-Section "IPv6 Enabled"
Get-NetAdapterBinding -ComponentID ms_tcpip6 | Format-Table -AutoSize

if ($Endpoint) {
    $endpointValue = $Endpoint.Trim()
    if ($endpointValue.StartsWith("[") -and $endpointValue.EndsWith("]")) {
        $endpointValue = $endpointValue.TrimStart("[").TrimEnd("]")
    }
    Write-Section "Endpoint Connectivity Test"
    Write-Output ("Endpoint: {0}:{1}" -f $endpointValue, $EndpointPort)
    try {
        Test-NetConnection -ComputerName $endpointValue -Port $EndpointPort | Format-List
    } catch {
        Write-Output ("Test-NetConnection failed: {0}" -f $_.Exception.Message)
    }
}

Write-Section "Route Print (Top)"
route print | Select-Object -First 80

try {
    Stop-Transcript | Out-Null
} catch {
    Write-Output "Failed to stop transcript."
}

Write-Output ("Saved diagnostics to {0}" -f (Resolve-Path $OutFile))
