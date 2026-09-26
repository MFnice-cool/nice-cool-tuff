# B3 headless two-node capsule gossip smoke for tuffswarm-node.
# Starts A+B, POSTs a signed capsule to A, asserts gossip.ok and B recv/count.
#
# Required:
#   TUFFSWARM_CONTROL_TOKEN  - Bearer for /v1/*
#
# Optional:
#   TUFFSWARM_NODE_BIN       - path to tuffswarm-node.exe
#   CAPSULE_SMOKE_KEEP=1     - leave nodes running after assert
#
# Usage (from repo root):
#   $env:TUFFSWARM_CONTROL_TOKEN = "smoke-token"
#   .\crates\tuffswarm-node\scripts\capsule-smoke.ps1

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
  throw "Set TUFFSWARM_CONTROL_TOKEN (Bearer for /v1/*) before running capsule-smoke.ps1"
}

$bin = Resolve-NodeBin
$root = Resolve-Path (Join-Path $PSScriptRoot "..\..\..")
$dataRoot = Join-Path $env:TEMP "tuffswarm-capsule-smoke"
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
  Write-Host "Building tuffswarm-node + smoke_sign_capsule ..."
  Push-Location $root
  try {
    cargo build -p tuffswarm-node --example smoke_sign_capsule --quiet
  } finally {
    Pop-Location
  }

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

  $deadlinePeers = (Get-Date).AddSeconds(25)
  $peersOk = $false
  while ((Get-Date) -lt $deadlinePeers) {
    $stA = Get-NodeStatus $baseA $token
    $stB = Get-NodeStatus $baseB $token
    if ([int]$stA.peers -ge 1 -and [int]$stB.peers -ge 1) {
      $peersOk = $true
      break
    }
    Start-Sleep -Milliseconds 500
  }
  if (-not $peersOk) {
    throw "Peers not connected before publish (A=$($stA.peers) B=$($stB.peers))"
  }

  $fp = "smoke|b3|$([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds())"
  Write-Host "Signing capsule (fingerprint=$fp) ..."
  $capsuleJson = (& cargo run -p tuffswarm-node --example smoke_sign_capsule --quiet -- --fingerprint $fp | Out-String).Trim()
  if (-not $capsuleJson) {
    throw "smoke_sign_capsule produced empty output"
  }

  $countA0 = [int](Get-NodeStatus $baseA $token).capsuleCount
  $countB0 = [int](Get-NodeStatus $baseB $token).capsuleCount
  $recvB0 = [int](Get-NodeStatus $baseB $token).gossipReceived

  Write-Host "POST /v1/crash/capsules to A ..."
  $headers = @{
    Authorization = "Bearer $token"
    "Content-Type" = "application/json; charset=utf-8"
  }
  # POST raw JSON from the signer — avoid PowerShell ConvertTo-Json mangling.
  $publish = Invoke-RestMethod -Method Post -Uri "$baseA/v1/crash/capsules" -Headers $headers -Body ([System.Text.Encoding]::UTF8.GetBytes($capsuleJson)) -TimeoutSec 15
  if (-not $publish.ok -or -not $publish.stored) {
    throw "Publish did not store capsule: $($publish | ConvertTo-Json -Compress)"
  }
  if (-not $publish.gossip -or $publish.gossip.ok -ne $true) {
    $gerr = $publish.gossip.error
    throw "Expected gossip.ok=true with peers connected; got error=$gerr"
  }

  $ok = $false
  $deadline = (Get-Date).AddSeconds(25)
  $stB = $null
  while ((Get-Date) -lt $deadline) {
    $stB = Get-NodeStatus $baseB $token
    $countB = [int]$stB.capsuleCount
    $recvB = [int]$stB.gossipReceived
    if ($countB -gt $countB0 -or $recvB -gt $recvB0) {
      $ok = $true
      break
    }
    Start-Sleep -Milliseconds 400
  }
  if (-not $ok) {
    throw "Peer B did not receive capsule (B count=$($stB.capsuleCount) recv=$($stB.gossipReceived); baselines count=$countB0 recv=$recvB0)"
  }

  $stA = Get-NodeStatus $baseA $token
  Write-Host ""
  Write-Host "PASS: capsule gossip (A pub=$($stA.gossipPublished) count=$($stA.capsuleCount); B recv=$($stB.gossipReceived) count=$($stB.capsuleCount))."
  Write-Host ""
  Write-Host "Next manual UI steps:"
  Write-Host "  1) Prefer P2P + peers>=1; Distill Confirm on A."
  Write-Host "  2) B Refresh status: Gossip recv / capsuleCount up; Explain may hit swarm_capsule."
  Write-Host "  Docs: docs/13-tuffswarm-network.md (B3 Manual capsule gossip checklist)."
  Write-Host ""
  if ($env:CAPSULE_SMOKE_KEEP -eq "1") {
    Write-Host "CAPSULE_SMOKE_KEEP=1 - leaving nodes running (PIDs $($script:procA.Id), $($script:procB.Id))."
  } else {
    Stop-SmokeNodes
    Write-Host "Nodes stopped. Set CAPSULE_SMOKE_KEEP=1 to leave them up."
  }
} catch {
  Stop-SmokeNodes
  Write-Error $_
  exit 1
}
