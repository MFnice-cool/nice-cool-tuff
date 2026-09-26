# B1 headless two-node LAN smoke for tuffswarm-node (Fog/Creation transport).
# Does NOT run Relay (--relay-server = B2). Does NOT exercise desktop UI Accept/Kudos.
#
# Required:
#   TUFFSWARM_CONTROL_TOKEN  - Bearer for /v1/* (same token on both nodes for this smoke)
#
# Optional:
#   TUFFSWARM_NODE_BIN       - path to tuffswarm-node.exe (else target/debug or release)
#   TUFFSWARM_DIAGNOSE_VOLUNTEER=1 / TUFFSWARM_CREATION_WORKER=1 - advertise capabilities
#   LAN_SMOKE_KEEP=1         - leave nodes running after assert
#
# Usage (from repo root):
#   $env:TUFFSWARM_CONTROL_TOKEN = "smoke-token"
#   .\crates\tuffswarm-node\scripts\lan-smoke.ps1

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
      $_ -notmatch '/ip4/0\.0\.0\.0/'
    })
  if ($preferred.Count -gt 0) { return $preferred[0] }
  $fallback = @($Addrs | Where-Object { $_ -notmatch '/ip4/0\.0\.0\.0/' })
  if ($fallback.Count -gt 0) { return $fallback[0] }
  return $Addrs[0]
}

$token = $env:TUFFSWARM_CONTROL_TOKEN
if (-not $token -or -not $token.Trim()) {
  throw "Set TUFFSWARM_CONTROL_TOKEN (Bearer for /v1/*) before running lan-smoke.ps1"
}

$bin = Resolve-NodeBin
$dataRoot = Join-Path $env:TEMP "tuffswarm-lan-smoke"
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
  Write-Host "Starting node A on 127.0.0.1:8790 ..."
  $env:TUFFSWARM_CONTROL_TOKEN = $token
  $script:procA = Start-Process -FilePath $bin -PassThru -WindowStyle Hidden -ArgumentList @(
    "--control", "127.0.0.1:8790",
    "--listen", "/ip4/0.0.0.0/tcp/0",
    "--data-dir", $aData
  )
  Wait-Healthy $baseA

  $boot = $null
  $deadlineListen = (Get-Date).AddSeconds(10)
  while ((Get-Date) -lt $deadlineListen) {
    $stA = Get-NodeStatus $baseA $token
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

  $ok = $false
  $deadline = (Get-Date).AddSeconds(25)
  $peersA = 0
  $peersB = 0
  $stA = $null
  $stB = $null
  while ((Get-Date) -lt $deadline) {
    $stA = Get-NodeStatus $baseA $token
    $stB = Get-NodeStatus $baseB $token
    $peersA = [int]$stA.peers
    $peersB = [int]$stB.peers
    if ($peersA -ge 1 -and $peersB -ge 1) {
      $ok = $true
      break
    }
    Start-Sleep -Milliseconds 500
  }

  if (-not $ok) {
    throw "Peer assert failed: A peers=$peersA B peers=$peersB (want both >= 1). Check firewall / bootstrap multiaddr."
  }

  $creA = @($stA.creationPeers).Count
  $creB = @($stB.creationPeers).Count
  $volA = @($stA.volunteerPeers).Count
  $volB = @($stB.volunteerPeers).Count

  Write-Host ""
  Write-Host "PASS: two-node LAN smoke healthy (A peers=$peersA, B peers=$peersB)."
  Write-Host "creationPeers A=$creA B=$creB; volunteerPeers A=$volA B=$volB"
  Write-Host "  (For capability ads in a real smoke, set TUFFSWARM_DIAGNOSE_VOLUNTEER=1 / TUFFSWARM_CREATION_WORKER=1 on the worker process.)"
  Write-Host ""
  Write-Host "Next manual UI steps:"
  Write-Host "  1) Fog: enable Fog volunteer + local AI on one PC; Explain unknown crash on the other -> l2_hit / swarm_volunteer."
  Write-Host "  2) Creation: enable Creation worker on worker PC; Trends submit -> verify -> Accept -> kudos.awarded."
  Write-Host "  3) If peers=0 in Settings: copy listenAddrs into Bootstrap peer multiaddr, then Start/attach."
  Write-Host "  Docs: docs/13-tuffswarm-network.md (B1 Manual LAN checklist)."
  Write-Host ""
  if ($env:LAN_SMOKE_KEEP -eq "1") {
    Write-Host "LAN_SMOKE_KEEP=1 - leaving nodes running (PIDs $($script:procA.Id), $($script:procB.Id))."
  } else {
    Stop-SmokeNodes
    Write-Host "Nodes stopped. Set LAN_SMOKE_KEEP=1 to leave them up."
  }
} catch {
  Stop-SmokeNodes
  Write-Error $_
  exit 1
}
