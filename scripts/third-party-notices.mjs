import { spawnSync } from 'node:child_process';
import { readFile, readdir, writeFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';

const root = resolve(import.meta.dirname, '..');
const result = spawnSync('cargo', ['metadata', '--format-version', '1', '--filter-platform', 'aarch64-apple-darwin'], {
  cwd: root, encoding: 'utf8', maxBuffer: 16 * 1024 * 1024,
});
if (result.error) throw result.error;
if (result.status !== 0) throw new Error(result.stderr);
const metadata = JSON.parse(result.stdout);
const resolved = new Set(metadata.resolve.nodes.map(node => node.id));
const notices = ['Sound Backup — third-party notices\n\nThese notices cover the resolved macOS dependency graph, including build tools.\n'];
for (const pkg of metadata.packages.sort((a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version))) {
  if (!resolved.has(pkg.id) || !pkg.source) continue;
  const directory = dirname(pkg.manifest_path);
  notices.push(`\n${'='.repeat(72)}\n${pkg.name} ${pkg.version} — ${pkg.license || 'see upstream'}\n${pkg.repository || ''}\n`);
  const entries = await readdir(directory, { withFileTypes: true });
  for (const entry of entries.filter(e => e.isFile() && /^(LICENSE|LICENCE|COPYING|COPYRIGHT|NOTICE)/.test(e.name)).sort((a, b) => a.name.localeCompare(b.name))) {
    notices.push(`\n${entry.name}\n${await readFile(resolve(directory, entry.name), 'utf8')}\n`);
  }
}
notices.push(`\n${'='.repeat(72)}\nFont Awesome Free 7.3.1\n${await readFile(resolve(root, 'node_modules/@fortawesome/fontawesome-free/LICENSE.txt'), 'utf8')}`);
await writeFile(resolve(root, 'THIRD_PARTY_NOTICES.txt'), notices.join(''));
