import { cp, mkdir, rm } from 'node:fs/promises';
import { resolve } from 'node:path';

const root = resolve(import.meta.dirname, '..');
const source = resolve(root, 'src-web');
const output = resolve(root, 'dist');
const fontAwesome = resolve(root, 'node_modules', '@fortawesome', 'fontawesome-free');

await rm(output, { recursive: true, force: true });
await mkdir(output, { recursive: true });
await cp(source, output, { recursive: true });
await mkdir(resolve(output, 'vendor', 'fontawesome', 'css'), { recursive: true });
for (const stylesheet of ['fontawesome.min.css', 'solid.min.css']) {
  await cp(
    resolve(fontAwesome, 'css', stylesheet),
    resolve(output, 'vendor', 'fontawesome', 'css', stylesheet)
  );
}
await mkdir(resolve(output, 'vendor', 'fontawesome', 'webfonts'), { recursive: true });
await cp(
  resolve(fontAwesome, 'webfonts', 'fa-solid-900.woff2'),
  resolve(output, 'vendor', 'fontawesome', 'webfonts', 'fa-solid-900.woff2')
);
console.log(`Built ${output}`);
