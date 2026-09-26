# B2 headless two-node Relay smoke for tuffswarm-node.
# Node A runs --relay-server; node B bootstraps A's listen multiaddr and should
# obtain a circuit listen addr and peer connectivity.
# Same-host is NOT a real NAT proof — validates wiring only.
#
# Required:
#   TUFFSWARM_CONTROL_TOKEN  - Bearer for /v1/* (same token on both nodes)
#
# Optional:
#   TUFFSWARM_NODE_BIN       - path to tuffswarm-node.exe
#   RELAY_SMOKE_KEEP=1       - leave nodes running after assert
#
# Usage (from repo root):
#   $env:TUFFSWARM_CONTROL_TOKEN = "smoke-token"
#   .\crates\tuffswarm-node\scripts\relay-smoke.ps1

$ErrorActionPreference = "Stop"

function Resolve-NodeBin {
  if ($env:TUFFSWARM_NODE_BIN -and (Test-Path $env:TUFFSWARM_NODE_BIN)) {
    return (Resolve-Path $env:TUFFSWARM_NODE_BIN).Path
  }
  $root = Resolve-Path (Join-Path $PSScriptRoot "..\..\..")
  foreach ($cand in @(
      (Join-Path $root "target\debug\tuffswarm-node.exe"),
      (Join-Path $root "target\release\tuffswarm-node.exe")
    )) {
    if (Test-Path $cand) { return $cand }
  }
  throw "tuffswarm-node.exe not found. Build with: cargo build -p tuffswarm-node (or set TUFFSWARM_NODE_BIN)."
}

function Wait-Healthy([string]$Base, [int]$TimeoutSec = 20) {
  $deadline = (Get-Date).AddSeconds($TimeoutSec)
  while ((Get-Date) -lt $deadline) {
    try {
      $r = Invoke-WebRequest -Uri "$Base/health" -UseBasicParsing -TimeoutSec 2
      if ($r.StatusCode -eq 200) { return }
    } catch {
      Start-Sleep -Milliseconds 400
    }
  }
  throw "Node not healthy at $Base within ${TimeoutSec}s"
}

function Get-NodeStatus([string]$Base, [string]$Token) {
  $headers = @{ Authorization = "Bearer $Token" }
  return Invoke-RestMethod -Uri "$Base/v1/node/status" -Headers $headers -TimeoutSec 5
}

function Pick-BootstrapAddr([string[]]$Addrs) {
  if (-not $Addrs -or $Addrs.Count -eq 0) { return $null }
  $preferred = @($Addrs | Where-Object {
      $_ -notmatch '/ip4/127\.0\.0\.1/' -and
      $_ -notmatch '/ip6/::1/' -and
      $_ -notmatch '/ip4/0\.0\.0\.0/' -and
      $_ -notmatch 'p2p-circuit'
    })
  if ($preferred.Count -gt 0) { return $preferred[0] }
  $fallback = @($Addrs | Where-Object {
      $_ -notmatch '/ip4/0\.0\.0\.0/' -and $_ -notmatch 'p2p-circuit'
    })
  if ($fallback.Count -gt 0) { return $fallback[0] }
  return $Addrs[0]
}

$token = $env:TUFFSWARM_CONTROL_TOKEN
if (-not $token -or -not $token.Trim()) {
  throw "Set TUFFSWARM_CONTROL_TOKEN (Bearer for /v1/*) before running relay-smoke.ps1"
}

$bin = Resolve-NodeBin
$dataRoot = Join-Path $env:TEMP "tuffswarm-relay-smoke"
$aData = Join-Path $dataRoot "node-a"
$bData = Join-Path $dataRoot "node-b"
New-Item -ItemType Directory -Force -Path $aData, $bData | Out-Null

$baseA = "http://127.0.0.1:8790"
$baseB = "http://127.0.0.1:8791"
$script:procA = $null
$script:procB = $null

function Stop-SmokeNodes {
  foreach ($p in @($script:procB, $script:procA)) {
    if ($null -ne $p -and -not $p.HasExited) {
      try { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue } catch {}
    }
  }
}

try {
  Write-Host "Using binary: $bin"
  Write-Host "Starting relay host A on 127.0.0.1:8790 (--relay-server) ..."
  $env:TUFFSWARM_CONTROL_TOKEN = $token
  $script:procA = Start-Process -FilePath $bin -PassThru -WindowStyle Hidden -ArgumentList @(
    "--control", "127.0.0.1:8790",
    "--listen", "/ip4/0.0.0.0/tcp/0",
    "--data-dir", $aData,
    "--relay-server"
  )
  Wait-Healthy $baseA

  $boot = $null
  $deadlineListen = (Get-Date).AddSeconds(10)
  while ((Get-Date) -lt $deadlineListen) {
    $stA = Get-NodeStatus $baseA $token
    if (-not $stA.relayServer) {
      throw "Node A status.relayServer is false - --relay-server not effective"
    }
    $boot = Pick-BootstrapAddr @($stA.listenAddrs)
    if ($boot) { break }
    Start-Sleep -Milliseconds 300
  }
  if (-not $boot) {
    throw "Node A has no usable listenAddrs yet - cannot bootstrap node B"
  }
  Write-Host "Node A listen (bootstrap): $boot"

  Write-Host "Starting node B on 127.0.0.1:8791 with --bootstrap ..."
  $script:procB = Start-Process -FilePath $bin -PassThru -WindowStyle Hidden -ArgumentList @(
    "--control", "127.0.0.1:8791",
    "--listen", "/ip4/0.0.0.0/tcp/0",
    "--data-dir", $bData,
    "--bootstrap", $boot
  )
  Wait-Healthy $baseB

  $okPeers = $false
  $okCircuit = $false
  $deadline = (Get-Date).AddSeconds(30)
  $peersA = 0
  $peersB = 0
  $circuitB = 0
  $stA = $null
  $stB = $null
  while ((Get-Date) -lt $deadline) {
    $stA = Get-NodeStatus $baseA $token
    $stB = Get-NodeStatus $baseB $token
    $peersA = [int]$stA.peers
    $peersB = [int]$stB.peers
    $circuitB = @($stB.circuitListenAddrs).Count
    if ($peersA -ge 1 -and $peersB -ge 1) { $okPeers = $true }
    if ($circuitB -ge 1) { $okCircuit = $true }
    if ($okPeers -and $okCircuit) { break }
    Start-Sleep -Milliseconds 500
  }

  if (-not $okPeers) {
    throw "Peer assert failed: A peers=$peersA B peers=$peersB (want both >= 1)."
  }
  if (-not $okCircuit) {
    Write-Host "WARN: B circuitListenAddrs still empty (peers ok). Reservation may need more time on this host."
  }

  Write-Host ""
  Write-Host "PASS: relay smoke (A peers=$peersA, B peers=$peersB, B circuitAddrs=$circuitB, A.relayServer=$($stA.relayServer))."
  Write-Host ""
  Write-Host "Next manual NAT/UI steps:"
  Write-Host "  1) Public host: Settings -> Act as Circuit Relay; Copy listen address."
  Write-Host "  2) NAT peer: paste into Bootstrap; Start/attach; expect Circuit hint / peers."
  Write-Host "  3) Optional: Fog L2 / Creation Accept across relay (same roles as B1)."
  Write-Host "  Docs: docs/13-tuffswarm-network.md (B2 Manual Relay / NAT checklist)."
  Write-Host ""
  if ($env:RELAY_SMOKE_KEEP -eq "1") {
    Write-Host "RELAY_SMOKE_KEEP=1 - leaving nodes running (PIDs $($script:procA.Id), $($script:procB.Id))."
  } else {
    Stop-SmokeNodes
    Write-Host "Nodes stopped. Set RELAY_SMOKE_KEEP=1 to leave them up."
  }
} catch {
  Stop-SmokeNodes
  Write-Error $_
  exit 1
}
