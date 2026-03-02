#Requires -Version 5.1
<#
.SYNOPSIS
    Interactive test runner for Ephemeris -- starts infrastructure, builds, runs, and tests.

.DESCRIPTION
    Launches Docker services, builds the Ephemeris binary (open-core or enterprise),
    starts the application, and walks you through sending test events via the REST API
    and MQTT. Cleans up on exit.

.EXAMPLE
    .\test-app.ps1                  # Interactive menu
    .\test-app.ps1 -Backend postgres # Skip menu, go straight to PostgreSQL mode
    .\test-app.ps1 -Backend arango   # Skip menu, go straight to ArangoDB enterprise mode
#>
param(
    [ValidateSet("postgres", "arango")]
    [string]$Backend
)

$ErrorActionPreference = "Continue"
$ProjectRoot = $PSScriptRoot
$ComposeFile = Join-Path $ProjectRoot "docker-compose.dev.yml"

# Ensure Cargo is in PATH (rustup default install location)
$CargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
if ((Test-Path $CargoBin) -and ($env:PATH -notlike "*$CargoBin*")) {
    $env:PATH = "$CargoBin;$env:PATH"
}

# --- Colors ---------------------------------------------------------------
function Write-Header($msg)  { Write-Host "`n=== $msg ===" -ForegroundColor Cyan }
function Write-Step($msg)    { Write-Host "  > $msg" -ForegroundColor Green }
function Write-Warn($msg)    { Write-Host "  ! $msg" -ForegroundColor Yellow }
function Write-Err($msg)     { Write-Host "  X $msg" -ForegroundColor Red }
function Write-Info($msg)    { Write-Host "    $msg" -ForegroundColor Gray }

# Helper: run an external command suppressing stderr display issues in PS.
# Captures exit code in $LASTEXITCODE.
function Invoke-Native {
    param([string]$Command)
    & cmd /c "$Command" 2>nul
}

# --- Prerequisites --------------------------------------------------------
function Test-Prerequisites {
    Write-Header "Checking prerequisites"

    $missing = @()
    foreach ($cmd in @("docker", "cargo", "curl")) {
        if (Get-Command $cmd -ErrorAction SilentlyContinue) {
            Write-Step "$cmd found"
        } else {
            Write-Err "$cmd not found"
            $missing += $cmd
        }
    }

    # Check Docker is running
    & cmd /c "docker info >nul 2>&1"
    if ($LASTEXITCODE -eq 0) {
        Write-Step "Docker daemon is running"
    } else {
        Write-Err "Docker daemon is not running. Please start Docker Desktop."
        $missing += "docker-daemon"
    }

    if ($missing.Count -gt 0) {
        Write-Err "Missing prerequisites: $($missing -join ', ')"
        Write-Host "Please install the missing tools and try again."
        exit 1
    }
}

# --- Docker Services ------------------------------------------------------
function Start-DockerServices([string]$mode) {
    Write-Header "Starting Docker services ($mode mode)"

    if ($mode -eq "postgres") {
        Write-Step "Starting PostgreSQL + Mosquitto..."
        & cmd /c "docker compose -f `"$ComposeFile`" up -d postgres mosquitto >nul 2>&1"
    } else {
        Write-Step "Starting PostgreSQL + Mosquitto + ArangoDB..."
        & cmd /c "docker compose -f `"$ComposeFile`" up -d >nul 2>&1"
    }

    # Wait for PostgreSQL
    Write-Step "Waiting for PostgreSQL to be ready..."
    $retries = 0
    while ($retries -lt 30) {
        & cmd /c "docker compose -f `"$ComposeFile`" exec -T postgres pg_isready -U ephemeris >nul 2>&1"
        if ($LASTEXITCODE -eq 0) {
            Write-Step "PostgreSQL is ready"
            break
        }
        Start-Sleep -Seconds 1
        $retries++
    }
    if ($retries -ge 30) {
        Write-Err "PostgreSQL failed to start within 30 seconds"
        exit 1
    }

    # Wait for ArangoDB if enterprise mode
    if ($mode -eq "arango") {
        Write-Step "Waiting for ArangoDB to be ready..."
        $retries = 0
        while ($retries -lt 30) {
            try {
                $resp = curl.exe -s -o NUL -w "%{http_code}" http://localhost:8529/_api/version 2>$null
                if ($resp -eq "200" -or $resp -eq "401") {
                    Write-Step "ArangoDB is ready"
                    break
                }
            } catch {}
            Start-Sleep -Seconds 1
            $retries++
        }
        if ($retries -ge 30) {
            Write-Err "ArangoDB failed to start within 30 seconds"
            exit 1
        }

        # Create the ephemeris database if it doesn't exist
        Write-Step "Ensuring 'ephemeris' database exists in ArangoDB..."
        $authBody = '{"username":"root","password":"ephemeris"}'
        $authResp = curl.exe -s -X POST http://localhost:8529/_open/auth -H "Content-Type: application/json" -d $authBody 2>$null | ConvertFrom-Json
        $jwt = $authResp.jwt

        $createBody = '{"name":"ephemeris"}'
        curl.exe -s -X POST http://localhost:8529/_api/database -H "Content-Type: application/json" -H "Authorization: bearer $jwt" -d $createBody 2>$null | Out-Null
        Write-Step "ArangoDB database 'ephemeris' is ready"
    }
}

function Stop-DockerServices {
    Write-Header "Stopping Docker services"
    & cmd /c "docker compose -f `"$ComposeFile`" down >nul 2>&1"
    Write-Step "Services stopped (volumes preserved -- use 'docker compose -f docker-compose.dev.yml down -v' to wipe)"
}

# --- Build ----------------------------------------------------------------
function Build-Ephemeris([string]$mode) {
    Write-Header "Building Ephemeris"

    Push-Location $ProjectRoot
    try {
        if ($mode -eq "arango") {
            Write-Step "Building with enterprise-arango feature..."
            # Run cargo via cmd to avoid PS stderr->ErrorRecord conversion
            & cmd /c "cargo build --features enterprise-arango 2>&1"
        } else {
            Write-Step "Building open-core (PostgreSQL only)..."
            & cmd /c "cargo build 2>&1"
        }

        if ($LASTEXITCODE -ne 0) {
            Write-Err "Build failed!"
            exit 1
        }
        Write-Step "Build succeeded"
    } finally {
        Pop-Location
    }
}

# --- Run App --------------------------------------------------------------
$script:AppProcess = $null

function Start-Ephemeris([string]$mode) {
    Write-Header "Starting Ephemeris"

    $configFile = if ($mode -eq "arango") {
        Join-Path $ProjectRoot "ephemeris-arango.toml"
    } else {
        Join-Path $ProjectRoot "ephemeris.toml"
    }

    Write-Step "Config: $configFile"
    Write-Step "API will be at: http://localhost:8080"

    $exePath = Join-Path $ProjectRoot "target\debug\ephemeris.exe"
    $script:AppLog = Join-Path $ProjectRoot "ephemeris.log"
    $script:AppProcess = Start-Process -FilePath $exePath `
        -ArgumentList "--config", $configFile `
        -WorkingDirectory $ProjectRoot `
        -RedirectStandardOutput $script:AppLog `
        -RedirectStandardError (Join-Path $ProjectRoot "ephemeris-err.log") `
        -PassThru

    # Wait for API to be ready
    Write-Step "Waiting for API server..."
    $retries = 0
    while ($retries -lt 20) {
        $health = curl.exe -s -o NUL -w "%{http_code}" http://localhost:8080/health 2>$null
        if ($health -eq "200") {
            Write-Step "Ephemeris is running! (PID: $($script:AppProcess.Id))"
            Write-Info "Log: $($script:AppLog)"
            return
        }
        if ($script:AppProcess.HasExited) {
            Write-Err "Ephemeris exited unexpectedly (exit code: $($script:AppProcess.ExitCode))"
            Write-Err "Check logs: $($script:AppLog)"
            exit 1
        }
        Start-Sleep -Seconds 1
        $retries++
    }
    Write-Err "Ephemeris failed to start within 20 seconds"
    Write-Err "Check logs: $($script:AppLog)"
    if ($script:AppProcess -and !$script:AppProcess.HasExited) {
        $script:AppProcess.Kill()
    }
    exit 1
}

function Stop-Ephemeris {
    if ($script:AppProcess -and !$script:AppProcess.HasExited) {
        Write-Step "Stopping Ephemeris (PID: $($script:AppProcess.Id))..."
        $script:AppProcess.Kill()
        $script:AppProcess.WaitForExit(5000)
    }
}

# --- Test Commands --------------------------------------------------------
$ApiBase = "http://localhost:8080"

function Test-HealthCheck {
    Write-Header "Health Check"
    $resp = curl.exe -s "$ApiBase/health" | ConvertFrom-Json
    Write-Step "Response: $($resp | ConvertTo-Json -Compress)"
}

function Send-ObjectEvent {
    Write-Header "Sending ObjectEvent (commissioning)"

    $serial = Get-Random -Minimum 1000 -Maximum 9999
    $epc = "urn:epc:id:sgtin:0614141.107346.$serial"
    $body = @"
{
    "type": "ObjectEvent",
    "action": "OBSERVE",
    "eventTime": "$(Get-Date -Format 'yyyy-MM-ddTHH:mm:ss.fffzzz')",
    "eventTimeZoneOffset": "$(Get-Date -Format 'zzz')",
    "epcList": ["$epc"],
    "bizStep": "urn:epcglobal:cbv:bizstep:commissioning",
    "disposition": "urn:epcglobal:cbv:disp:active"
}
"@

    Write-Info "EPC: $epc"
    Write-Info "bizStep: commissioning"

    $resp = $body | curl.exe -s -X POST "$ApiBase/events" -H "Content-Type: application/json" -d "@-"
    Write-Step "Capture response: $resp"

    # Check SN state
    Start-Sleep -Milliseconds 200
    $snResp = curl.exe -s "$ApiBase/serial-numbers/$epc"
    Write-Step "SN state: $snResp"

    return $epc
}

function Send-AggregationEvent([string]$parentEpc, [string[]]$childEpcs) {
    Write-Header "Sending AggregationEvent (packing)"

    if (-not $parentEpc) {
        $parentSerial = Get-Random -Minimum 100000 -Maximum 999999
        $parentEpc = "urn:epc:id:sscc:0614141.$parentSerial"
    }

    if (-not $childEpcs -or $childEpcs.Count -eq 0) {
        $childEpcs = @(
            "urn:epc:id:sgtin:0614141.107346.$(Get-Random -Minimum 1000 -Maximum 9999)",
            "urn:epc:id:sgtin:0614141.107346.$(Get-Random -Minimum 1000 -Maximum 9999)",
            "urn:epc:id:sgtin:0614141.107346.$(Get-Random -Minimum 1000 -Maximum 9999)"
        )
    }

    $childJson = ($childEpcs | ForEach-Object { "`"$_`"" }) -join ", "
    $body = @"
{
    "type": "AggregationEvent",
    "action": "ADD",
    "eventTime": "$(Get-Date -Format 'yyyy-MM-ddTHH:mm:ss.fffzzz')",
    "eventTimeZoneOffset": "$(Get-Date -Format 'zzz')",
    "parentID": "$parentEpc",
    "childEPCs": [$childJson],
    "bizStep": "urn:epcglobal:cbv:bizstep:packing"
}
"@

    Write-Info "Parent: $parentEpc"
    Write-Info "Children: $($childEpcs -join ', ')"

    $resp = $body | curl.exe -s -X POST "$ApiBase/events" -H "Content-Type: application/json" -d "@-"
    Write-Step "Capture response: $resp"

    # Check hierarchy
    Start-Sleep -Milliseconds 200
    $hierResp = curl.exe -s "$ApiBase/hierarchy/$parentEpc/children"
    Write-Step "Children via API: $hierResp"

    return $parentEpc
}

function Send-ShippingEvent([string]$epc) {
    Write-Header "Sending ObjectEvent (shipping)"

    if (-not $epc) {
        $epc = "urn:epc:id:sgtin:0614141.107346.$(Get-Random -Minimum 1000 -Maximum 9999)"
    }

    $body = @"
{
    "type": "ObjectEvent",
    "action": "OBSERVE",
    "eventTime": "$(Get-Date -Format 'yyyy-MM-ddTHH:mm:ss.fffzzz')",
    "eventTimeZoneOffset": "$(Get-Date -Format 'zzz')",
    "epcList": ["$epc"],
    "bizStep": "urn:epcglobal:cbv:bizstep:shipping",
    "disposition": "urn:epcglobal:cbv:disp:in_transit"
}
"@

    Write-Info "EPC: $epc"
    Write-Info "bizStep: shipping (transitions to Released)"

    $resp = $body | curl.exe -s -X POST "$ApiBase/events" -H "Content-Type: application/json" -d "@-"
    Write-Step "Capture response: $resp"

    Start-Sleep -Milliseconds 200
    $snResp = curl.exe -s "$ApiBase/serial-numbers/$epc"
    Write-Step "SN state: $snResp"
}

function Send-ManualOverride([string]$epc, [string]$targetState, [string]$reason) {
    Write-Header "Manual State Override"

    if (-not $epc) { $epc = Read-Host "  Enter EPC" }
    if (-not $targetState) { $targetState = Read-Host "  Target state (e.g. destroyed, inactive)" }
    if (-not $reason) { $reason = Read-Host "  Reason" }

    $body = @"
{"targetState": "$targetState", "reason": "$reason"}
"@

    $resp = $body | curl.exe -s -X POST "$ApiBase/serial-numbers/$epc/transition" -H "Content-Type: application/json" -d "@-"
    Write-Step "Response: $resp"
}

function Query-Events {
    Write-Header "Querying All Events"
    $resp = curl.exe -s "$ApiBase/events"
    $events = $resp | ConvertFrom-Json
    Write-Step "Found $($events.Count) event(s)"
    foreach ($e in $events) {
        $type = $e.type
        $biz = if ($e.bizStep) { $e.bizStep } else { "(none)" }
        Write-Info "  $type | bizStep=$biz | time=$($e.eventTime)"
    }
}

function Query-SerialNumbers {
    Write-Header "Querying Serial Numbers"
    $resp = curl.exe -s "$ApiBase/serial-numbers"
    $sns = $resp | ConvertFrom-Json
    Write-Step "Found $($sns.Count) serial number(s)"
    foreach ($sn in $sns) {
        Write-Info "  $($sn.epc) | state=$($sn.state) | updated=$($sn.updated_at)"
    }
}

function Send-MqttEvent {
    Write-Header "Sending Event via MQTT"

    $serial = Get-Random -Minimum 1000 -Maximum 9999
    $body = "{`"type`":`"ObjectEvent`",`"action`":`"OBSERVE`",`"eventTime`":`"$(Get-Date -Format 'yyyy-MM-ddTHH:mm:ss.fffzzz')`",`"eventTimeZoneOffset`":`"$(Get-Date -Format 'zzz')`",`"epcList`":[`"urn:epc:id:sgtin:0614141.107346.$serial`"],`"bizStep`":`"urn:epcglobal:cbv:bizstep:commissioning`"}"
    $topic = "plant/line1/events/object"
    Write-Info "Topic: $topic"
    Write-Info "Payload: $body"

    # Check if mosquitto_pub is available locally
    if (Get-Command "mosquitto_pub" -ErrorAction SilentlyContinue) {
        mosquitto_pub -h localhost -p 1883 -t $topic -m $body
    } else {
        # Fall back to docker exec
        Write-Warn "mosquitto_pub not found locally, using Docker container..."
        & cmd /c "docker compose -f `"$ComposeFile`" exec -T mosquitto mosquitto_pub -t $topic -m `"$body`" 2>&1"
    }

    if ($LASTEXITCODE -eq 0) {
        Write-Step "Event published to MQTT"
    } else {
        Write-Err "Failed to publish MQTT event"
    }
}

function Run-FullPipelineDemo {
    Write-Header "FULL PIPELINE DEMO"
    Write-Host "  This will: commission 3 items, pack them into a case, then ship the case.`n" -ForegroundColor White

    # 1. Commission 3 items
    $epcs = @()
    for ($i = 0; $i -lt 3; $i++) {
        $epc = Send-ObjectEvent
        $epcs += $epc
        Start-Sleep -Milliseconds 100
    }

    # 2. Pack them into a case
    $caseEpc = "urn:epc:id:sscc:0614141.CASE$(Get-Random -Minimum 100 -Maximum 999)"
    Send-AggregationEvent -parentEpc $caseEpc -childEpcs $epcs

    # 3. Ship the case
    Send-ShippingEvent -epc $caseEpc

    # 4. Show final state
    Write-Header "Final State"
    Query-Events
    Query-SerialNumbers

    # 5. Show hierarchy
    Write-Header "Packaging Hierarchy for $caseEpc"
    $tree = curl.exe -s "$ApiBase/hierarchy/$caseEpc" | ConvertFrom-Json
    Write-Step "Root: $($tree.root)"
    foreach ($node in $tree.nodes) {
        Write-Info "  Child: $($node.epc)"
        if ($node.children) {
            foreach ($child in $node.children) {
                Write-Info "    Grandchild: $($child.epc)"
            }
        }
    }
}

# --- Interactive Menu -----------------------------------------------------
function Show-Menu {
    Write-Host ""
    Write-Host "  +---------------------------------------+" -ForegroundColor DarkCyan
    Write-Host "  |      Ephemeris Test Console            |" -ForegroundColor DarkCyan
    Write-Host "  +---------------------------------------+" -ForegroundColor DarkCyan
    Write-Host "  |  1) Health check                      |" -ForegroundColor White
    Write-Host "  |  2) Send ObjectEvent (commission)     |" -ForegroundColor White
    Write-Host "  |  3) Send ObjectEvent (shipping)       |" -ForegroundColor White
    Write-Host "  |  4) Send AggregationEvent (packing)   |" -ForegroundColor White
    Write-Host "  |  5) Manual state override             |" -ForegroundColor White
    Write-Host "  |  6) Query all events                  |" -ForegroundColor White
    Write-Host "  |  7) Query serial numbers              |" -ForegroundColor White
    Write-Host "  |  8) Send event via MQTT               |" -ForegroundColor White
    Write-Host "  |  9) Run full pipeline demo            |" -ForegroundColor Yellow
    Write-Host "  |  0) Quit                              |" -ForegroundColor White
    Write-Host "  +---------------------------------------+" -ForegroundColor DarkCyan
    Write-Host ""
}

# --- Main -----------------------------------------------------------------
function Main {
    Write-Host ""
    Write-Host "  Saturnis Ephemeris -- Interactive Test Runner" -ForegroundColor Cyan
    Write-Host "  ============================================" -ForegroundColor Cyan

    Test-Prerequisites

    # Choose backend
    if (-not $Backend) {
        Write-Host ""
        Write-Host "  Choose backend:" -ForegroundColor White
        Write-Host "    1) PostgreSQL (open-core, default)" -ForegroundColor Green
        Write-Host "    2) ArangoDB  (enterprise, requires Docker)" -ForegroundColor Yellow
        Write-Host ""
        $choice = Read-Host "  Selection [1]"
        $Backend = if ($choice -eq "2") { "arango" } else { "postgres" }
    }

    Write-Header "Mode: $Backend"

    try {
        Start-DockerServices $Backend
        Build-Ephemeris $Backend
        Start-Ephemeris $Backend

        # Interactive loop
        $quit = $false
        while (-not $quit) {
            Show-Menu
            $choice = Read-Host "  Choose [1-9, 0 to quit]"

            switch ($choice) {
                "1" { Test-HealthCheck }
                "2" { Send-ObjectEvent | Out-Null }
                "3" { Send-ShippingEvent }
                "4" { Send-AggregationEvent | Out-Null }
                "5" { Send-ManualOverride }
                "6" { Query-Events }
                "7" { Query-SerialNumbers }
                "8" { Send-MqttEvent }
                "9" { Run-FullPipelineDemo }
                "0" { $quit = $true }
                default { Write-Warn "Invalid choice" }
            }
        }
    } finally {
        Stop-Ephemeris
        Write-Host ""
        $cleanup = Read-Host "  Stop Docker services? [Y/n]"
        if ($cleanup -ne "n") {
            Stop-DockerServices
        } else {
            Write-Info "Docker services left running. Stop with: docker compose -f docker-compose.dev.yml down"
        }
        Write-Host ""
        Write-Step "Done!"
    }
}

Main
