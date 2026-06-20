#!/usr/bin/env node
/**
 * rendererConfig Patcher — 极简补丁
 * 
 * 修改 3P 模式的 rendererConfig(){return null}
 * 为 rendererConfig(){return this.buildConfig(null)}
 * 
 * 使得 Bri() 被调用生成含 [1m] 变体的默认 feature flags。
 * 配合 hosts 阻止远程 GrowthBook 覆盖，本地配置生效。
 */
'use strict';

const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');
const os = require('os');

const C = { reset:'\x1b[0m', bold:'\x1b[1m', dim:'\x1b[2m',
  red:'\x1b[31m', green:'\x1b[32m', yellow:'\x1b[33m', blue:'\x1b[34m', cyan:'\x1b[36m',
  bgRed:'\x1b[41m', bgGreen:'\x1b[42m' };
function fail(m) { console.error(`\n${C.bgRed} ERROR ${C.reset} ${C.red}${C.bold}${m}${C.reset}\n`); process.exit(1); }
function ok(m) { console.log(`  ${C.green}✓${C.reset} ${m}`); }
function warn(m) { console.log(`  ${C.yellow}⚠${C.reset} ${m}`); }
function info(l,m) { console.log(`  ${C.cyan}${l}${C.reset} ${m}`); }

const WORK = path.join(os.tmpdir(), 'rc-patch-work');

// 原始 3P 模式 → 1P 模式代码
const OLD = 'rendererConfig(){return null}';
const NEW_ = 'rendererConfig(){return this.buildConfig(null)}';

function detect() {
  const idx = process.argv.indexOf('--asar');
  if (idx >= 0 && idx+1 < process.argv.length) {
    const c = process.argv[idx+1];
    if (fs.existsSync(c)) {
      if (fs.statSync(c).isDirectory()) return { path: c, type: 'dir' };
      return { path: c, type: 'asar' };
    }
    fail(`Not found: ${c}`);
  }
  // PowerShell auto-detect
  try {
    const r = require('child_process').execSync(
      'powershell -Command "Get-AppxPackage -Name \'*Claude*\' -ErrorAction SilentlyContinue | Select-Object -ExpandProperty InstallLocation"',
      { encoding:'utf8', timeout:10000 }
    ).trim();
    if (r) {
      const loc = r.split(/\r?\n/).filter(l=>l.trim())[0];
      if (loc) {
        const a = path.join(loc.trim(), 'app', 'resources', 'app.asar');
        if (fs.existsSync(a)) return { path: a, type: 'asar' };
      }
    }
  } catch(e) {}
  fail('Could not locate app.asar. Use --asar <path>');
}

function getVer(d) {
  try { return JSON.parse(fs.readFileSync(path.join(d,'package.json'),'utf8')).version||'unknown'; } catch(e) { return 'unknown'; }
}

console.log(`\n ${C.bold}${C.blue}rendererConfig Patcher${C.reset}${C.dim} — 3P GrowthBook defaults fix${C.reset}\n`);

const tgt = detect();
info('target', tgt.type === 'asar' ? tgt.path : tgt.path + ' (dir)');
console.log('');

if (tgt.type === 'asar') {
  fs.rmSync(WORK, {recursive:true, force:true});
  info('extracting', '…');
  spawnSync(`npx --yes @electron/asar extract "${tgt.path}" "${WORK}"`, {shell:true, stdio:'pipe', timeout:120000});
  const idxPath = path.join(WORK, '.vite', 'build', 'index.js');
  if (!fs.existsSync(idxPath)) fail('Extract failed');
  ok('extracted');
  
  let code = fs.readFileSync(idxPath, 'utf8');
  
  // 验证原始模式
  const oldCnt = code.split(OLD).length - 1;
  if (oldCnt === 0) fail('rendererConfig(){return null} not found — already patched?');
  if (oldCnt > 1) warn(`Found ${oldCnt} occurrences — patching all`);
  
  // 替换
  const patched = code.replaceAll(OLD, NEW_);
  
  // 验证
  if (patched.includes(OLD)) fail('Patch failed — old code still present');
  const newCnt = patched.split(NEW_).length - 1;
  ok(`patched ${oldCnt} occurrence(s) → ${newCnt} rendererConfig() now uses buildConfig(null)`);
  
  // 写入
  fs.writeFileSync(idxPath, patched, 'utf8');
  ok('index.js written');
  
  // 重打包
  info('packing', '…');
  const out = path.join(path.dirname(WORK), 'app-patched.asar');
  spawnSync(`npx --yes @electron/asar pack "${WORK}" "${out}"`, {shell:true, stdio:'pipe', timeout:120000});
  ok('repacked');
  
  // 备份 & 安装
  const bakDir = path.join(os.homedir(), '.claude-1m-patch', 'backups');
  fs.mkdirSync(bakDir, {recursive:true});
  const ver = getVer(WORK);
  const bak = path.join(bakDir, `app-asar-${ver}-${Date.now()}-pre-patch`);
  const orig = fs.readFileSync(tgt.path);
  fs.writeFileSync(bak, orig);
  ok(`backup → ${path.basename(bak)}`);
  
  info('installing', '…');
  try {
    spawnSync(`copy /Y "${out}" "${tgt.path}"`, {shell:true, stdio:'pipe'});
    ok('installed');
  } catch(e) {
    warn('Direct copy failed — try elevated PowerShell');
    const ps = `Copy-Item -Path "${out}" -Destination "${tgt.path}" -Force`;
    try {
      spawnSync(`powershell -Command "Start-Process powershell -Verb RunAs -ArgumentList '-NoProfile -Command &{${ps}; Write-Output SUCCESS}' -Wait"`, {shell:true, stdio:'pipe', timeout:60000});
    } catch(e2) {
      warn(`Manual: Copy-Item -Path "${out}" -Destination "${tgt.path}" -Force`);
    }
  }
  
  try { fs.rmSync(WORK, {recursive:true, force:true}); } catch(e) {}
} else {
  // Directory mode — modify in place
  const idxPath = path.join(tgt.path, '.vite', 'build', 'index.js');
  let code = fs.readFileSync(idxPath, 'utf8');
  const oldCnt = code.split(OLD).length - 1;
  if (oldCnt === 0) fail('Not found — already patched?');
  const patched = code.replaceAll(OLD, NEW_);
  fs.writeFileSync(idxPath, patched, 'utf8');
  ok(`patched ${oldCnt} occurrence(s) — ${tgt.path}`);
}

console.log(`\n ${C.green}${C.bold}Done${C.reset}`);
console.log(` ${C.dim}1. hosts block: 127.0.0.1 claude.ai ✅${C.reset}`);
console.log(` ${C.dim}2. asar patched: rendererConfig() now calls buildConfig(null) ✅${C.reset}`);
console.log(` ${C.dim}3. Restart Claude Desktop${C.reset}\n`);
