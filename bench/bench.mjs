// Compares the three Doodoro builds, GPUI direct, gpuix-svelte and Tauri: bundle size, time until
// the window is on screen, memory and idle CPU. macOS only. Copied from ../doodoro-tauri/bench,
// with this app added.
//
//   node bench/bench.mjs [--runs 10] [--idle 30] [--hold]
//
// --hold waits for Enter before the CPU sample, so you can start a session in each app first.
// Each app runs with DOODORO_SETTINGS pointed at a temp file, so your own settings are left alone.

import { execFileSync, spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { createInterface } from 'node:readline/promises';
import { fileURLToPath } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const arg = (name, fallback) => {
	const i = process.argv.indexOf(`--${name}`);
	return i > 0 ? Number(process.argv[i + 1]) : fallback;
};
const RUNS = arg('runs', 10);
const IDLE_S = arg('idle', 30);
const HOLD = process.argv.includes('--hold');

const APPS = [
	{ name: 'GPUI', app: join(HERE, '../dist/Doodoro GPUI.app'), exe: 'Contents/MacOS/doodoro' },
	{ name: 'gpuix-svelte', app: join(HERE, '../../gpuix-podo/dist/Doodoro.app'), exe: 'Contents/MacOS/app' },
	{ name: 'Tauri', app: join(HERE, '../../doodoro-tauri/src-tauri/target/release/bundle/macos/Doodoro Tauri.app'), exe: 'Contents/MacOS/doodoro' }
];

const LAUNCH = join(HERE, '.launch');
if (!existsSync(LAUNCH) || statSync(LAUNCH).mtimeMs < statSync(join(HERE, 'launch.swift')).mtimeMs) {
	execFileSync('swiftc', ['-O', join(HERE, 'launch.swift'), '-o', LAUNCH]);
}

const temp = mkdtempSync(join(tmpdir(), 'doodoro-bench-'));
const env = { ...process.env, DOODORO_SETTINGS: join(temp, 'settings.json') };
const sh = (cmd, args) => execFileSync(cmd, args, { encoding: 'utf8' });
const mb = (bytes) => (bytes / 1024 / 1024).toFixed(1);

// WebKit runs a WKWebView's page, GPU work and networking in XPC services that launchd starts,
// not as the app's children; the ones that appear with the app are counted as the app's.
const webkit_pids = () =>
	new Set(
		sh('ps', ['-axo', 'pid=,command='])
			.split('\n')
			.filter((l) => l.includes('/WebKit.framework/') && l.includes('.xpc/'))
			.map((l) => Number(l.trim().split(/\s+/)[0]))
	);

function launch(exe) {
	const before = webkit_pids();
	const out = spawnSync(LAUNCH, [exe], { env, encoding: 'utf8' }).stdout.trim();
	const [pid, ms] = out.split(' ');
	if (ms === 'timeout') throw new Error(`${exe}: no window within 20 s`);
	return { pid: Number(pid), ms: Number(ms), before };
}

// Other apps' WebKit services can start meanwhile; only those macOS holds the same process
// responsible for as the app are its own.
function helpers(pid, before) {
	const fresh = [...webkit_pids()].filter((p) => !before.has(p));
	const owner = new Map(
		sh(LAUNCH, ['--responsible', String(pid), ...fresh.map(String)])
			.trim()
			.split('\n')
			.map((l) => l.split(' ').map(Number))
	);
	return fresh.filter((p) => owner.get(p) === owner.get(pid));
}

function quit(pid) {
	try {
		process.kill(pid);
	} catch {}
	// Let the app and its WebKit services go before the next launch.
	spawnSync('sleep', ['1']);
}

const footprint = (pid) => {
	const m = sh('footprint', ['-f', 'bytes', '--noCategories', '-p', String(pid)]).match(/Footprint: (\d+) B/);
	return m ? Number(m[1]) : 0;
};
const rss = (pid) => Number(sh('ps', ['-o', 'rss=', '-p', String(pid)]).trim()) * 1024;
/** CPU time so far, or null once the process has gone. */
const cpu_seconds = (pid) => {
	const t = spawnSync('ps', ['-o', 'time=', '-p', String(pid)], { encoding: 'utf8' }).stdout.trim();
	return t ? t.split(':').reduce((acc, part) => acc * 60 + Number(part), 0) : null;
};
const process_name = (pid) => sh('ps', ['-o', 'comm=', '-p', String(pid)]).trim().split('/').pop();

const median = (xs) => {
	const s = [...xs].sort((a, b) => a - b);
	return s.length % 2 ? s[(s.length - 1) / 2] : (s[s.length / 2 - 1] + s[s.length / 2]) / 2;
};

const results = [];
for (const a of APPS) {
	if (!existsSync(a.app)) {
		console.log(`skip ${a.name}: ${a.app} not built`);
		continue;
	}
	const exe = join(a.app, a.exe);
	console.log(`\n== ${a.name}`);

	const zip = join(temp, `${a.name}.zip`);
	sh('ditto', ['-c', '-k', '--keepParent', a.app, zip]);
	const size = {
		app: Number(sh('du', ['-sk', a.app]).split('\t')[0]) * 1024,
		executable: statSync(exe).size,
		zip: statSync(zip).size
	};
	console.log(`size: .app ${mb(size.app)} MB, executable ${mb(size.executable)} MB, zipped ${mb(size.zip)} MB`);

	// Two launches first, so the first-run checks macOS does on a new binary don't count.
	for (let i = 0; i < 2; i++) quit(launch(exe).pid);
	const startup = [];
	for (let i = 0; i < RUNS; i++) {
		const { pid, ms } = launch(exe);
		startup.push(ms);
		quit(pid);
	}
	console.log(`window on screen: median ${median(startup)} ms (${startup.join(', ')})`);

	const { pid, before } = launch(exe);
	spawnSync('sleep', ['5']);
	const pids = [pid, ...helpers(pid, before)];
	const memory = pids.map((p) => ({ name: process_name(p), footprint: footprint(p), rss: rss(p) }));
	for (const m of memory) console.log(`  ${m.name}: footprint ${mb(m.footprint)} MB, RSS ${mb(m.rss)} MB`);
	const total = {
		footprint: memory.reduce((s, m) => s + m.footprint, 0),
		rss: memory.reduce((s, m) => s + m.rss, 0)
	};
	console.log(`memory, ${pids.length} process(es): footprint ${mb(total.footprint)} MB, RSS ${mb(total.rss)} MB`);

	if (HOLD) {
		const rl = createInterface({ input: process.stdin, output: process.stdout });
		await rl.question(`Set ${a.name} up as you want it measured (e.g. start a session), then press Enter… `);
		rl.close();
	}
	const t0 = pids.map(cpu_seconds);
	spawnSync('sleep', [String(IDLE_S)]);
	const t1 = pids.map(cpu_seconds);
	const gone = pids.filter((_, i) => t0[i] === null || t1[i] === null);
	if (gone.length) console.log(`  left out, exited during the sample: ${gone.join(', ')}`);
	const used = pids.reduce((s, _, i) => (t0[i] === null || t1[i] === null ? s : s + t1[i] - t0[i]), 0);
	const cpu = (100 * used) / IDLE_S;
	console.log(`CPU over ${IDLE_S} s: ${cpu.toFixed(2)}% of one core`);
	quit(pid);

	results.push({ name: a.name, size, startup_ms: { median: median(startup), runs: startup }, memory, total, cpu_percent: cpu });
}

writeFileSync(join(HERE, 'results.json'), JSON.stringify({ date: new Date().toISOString(), hold: HOLD, results }, null, '\t') + '\n');
rmSync(temp, { recursive: true, force: true });

console.log('\n| | ' + results.map((r) => r.name).join(' | ') + ' |');
console.log('|---|' + results.map(() => '---|').join(''));
const row = (label, f) => console.log(`| ${label} | ${results.map(f).join(' | ')} |`);
row('.app size', (r) => `${mb(r.size.app)} MB`);
row('executable', (r) => `${mb(r.size.executable)} MB`);
row('zipped', (r) => `${mb(r.size.zip)} MB`);
row('window on screen (median)', (r) => `${r.startup_ms.median} ms`);
row('processes', (r) => r.memory.length);
row('memory (footprint)', (r) => `${mb(r.total.footprint)} MB`);
row('memory (RSS)', (r) => `${mb(r.total.rss)} MB`);
row(HOLD ? 'CPU (your setup)' : 'CPU, idle', (r) => `${r.cpu_percent.toFixed(2)}%`);
