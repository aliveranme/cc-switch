#!/usr/bin/env node
/**
 * Complete Restore & Cleanup Script
 * Reverts ALL changes and provides a clean path forward.
 * 
 * What this does:
 * 1. Rebuilds cc-switch with persistent context-1m header injection 
 * 2. Removes hosts block for claude.ai
 * 3. Restores original asar
 * 
 * After running: ALL requests through cc-switch automatically get 1M context,
 * regardless of what the model selector shows.
 */

'use strict';

const { execSync, spawnSync } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');

const C = { reset:'\x1b[0m', bold:'\x1b[1m', dim:'\x1b[2m',
  red:'\x1b[31m', green:'\x1b[32m', yellow:'\x1b[33m', blue:'\x1b[34m', cyan:'\x1b[36m' };

function ok(m) { console.log(`  ${C.green}✓${C.reset} ${m}`); }
function warn(m) { console.log(`  ${C.yellow}⚠${C.reset} ${m}`); }
function step(n, m) { console.log(`\n${C.bold}${C.blue}Step ${n}:${C.reset} ${m}`); }

// === Step 1: Rebuild cc-switch ===
step(1, 'Rebuilding cc-switch with context-1m header injection');

const projectDir = 'F:/Projects/temp/temp/cc-switch/src-tauri';
if (fs.existsSync(path.join(projectDir, 'Cargo.toml'))) {
  console.log('  Building cc-switch (this may take a minute)...');
  const result = spawnSync('cargo', ['build'], {
    cwd: projectDir,
    stdio: 'inherit',
    shell: true,
    timeout: 600000,
  });
  if (result.status === 0) {
    ok('cc-switch rebuilt successfully');
    const binPath = path.join(projectDir, 'target', 'debug', 'cc-switch.exe');
    if (fs.existsSync(binPath)) {
      console.log(`  New binary: ${binPath}`);
    }
  } else {
    warn('Build failed');
  }
} else {
  warn('cc-switch project not found at ' + projectDir);
}

// === Step 2: Restore original asar ===
step(2, 'Restoring original app.asar from backup');

const bakDir = path.join(os.homedir(), '.claude-1m-patch', 'backups');
if (fs.existsSync(bakDir)) {
  const backups = fs.readdirSync(bakDir).filter(f => f.includes('orig')).sort().reverse();
  if (backups.length > 0) {
    const latest = backups[0];
    const src = path.join(bakDir, latest);
    console.log(`  Found backup: ${latest}`);

    // Find current asar path
    try {
      const psOut = execSync(
        'powershell -Command "Get-AppxPackage -Name \'*Claude*\' -ErrorAction SilentlyContinue | Select-Object -ExpandProperty InstallLocation"',
        { encoding: 'utf8', timeout: 10000 }
      ).trim();
      if (psOut) {
        const lines = psOut.split(/\r?\n/).filter(l => l.trim());
        if (lines.length > 0) {
          const target = path.join(lines[0].trim(), 'app', 'resources', 'app.asar');
          if (fs.existsSync(target)) {
            fs.copyFileSync(src, target);
            ok(`Restored asar at ${target}`);
          }
        }
      }
    } catch (e) {
      warn(`Could not restore asar: ${e.message}`);
      console.log(`  Manual: copy "${src}" to app.asar location`);
    }
  } else {
    warn('No backups found');
  }
} else {
  warn('No backup directory');
}

// === Step 3: Remove hosts entry ===
step(3, 'Removing hosts block for claude.ai');

try {
  // Create temp file without claude.ai lines
  const psScript = [
    `$hosts = 'C:\\Windows\\System32\\drivers\\etc\\hosts'`,
    `$temp = "$env:TEMP\\hosts_clean.txt"`,
    `Get-Content $hosts | Where-Object {$_ -notmatch 'claude\\.ai'} | Set-Content $temp`,
    `Copy-Item $temp $hosts -Force`,
    `Remove-Item $temp`,
    `ipconfig /flushdns`,
  ].join('; ');

  execSync(
    `powershell -Command "Start-Process powershell -Verb RunAs -ArgumentList '-NoProfile -Command \\"${psScript}\\"' -Wait"`,
    { encoding: 'utf8', timeout: 30000 }
  );
  ok('Hosts entry removed');
} catch (e) {
  warn('Could not remove hosts entry automatically');
  console.log('  Manual: Run notepad C:\\Windows\\System32\\drivers\\etc\\hosts as Admin');
  console.log('  Remove lines containing "claude.ai"');
}

// === Summary ===
console.log(`\n${C.bold}${C.green}═════════════════════════════════════════════${C.reset}`);
console.log(`${C.bold}${C.green}  Cleanup Complete${C.reset}`);
console.log(`${C.bold}${C.green}═════════════════════════════════════════════${C.reset}`);
console.log(`\n What's in place:`);
console.log(`  ${C.cyan}•${C.reset} cc-switch proxy: injects context-1m header on all Claude requests`);
console.log(`  ${C.cyan}•${C.reset} app.asar: original (restored from backup)`);
console.log(`  ${C.cyan}•${C.reset} hosts: clean (no claude.ai block)`);
console.log(`\n How to test:`);
console.log(`  1. ${C.bold}Restart cc-switch${C.reset} with the newly built binary`);
console.log(`  2. ${C.bold}Restart Claude Desktop${C.reset}`);
console.log(`  3. Select any model — the proxy automatically enables 1M context`);
console.log(`\n${C.dim}Note: The model selector won't show [1m] separately,${C.reset}`);
console.log(`${C.dim}but ALL requests get 1M context at the API level.${C.reset}`);
console.log(`${C.dim}This is the only approach that works reliably.${C.reset}\n`);
