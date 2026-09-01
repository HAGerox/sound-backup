import http from 'node:http';
import { readFile, stat } from 'node:fs/promises';
import { extname, isAbsolute, join, normalize, relative, resolve } from 'node:path';
import './build.mjs';

const root = resolve(import.meta.dirname, '..', 'dist');
const port = 1420;
const types = {
  '.html': 'text/html; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.svg': 'image/svg+xml'
};

http.createServer(async (request, response) => {
  try {
    const urlPath = decodeURIComponent((request.url || '/').split('?')[0]);
    const requestedPath = normalize(urlPath === '/' ? 'index.html' : urlPath.replace(/^\/+/, ''));
    const path = join(root, requestedPath);
    const pathFromRoot = relative(root, path);
    if (pathFromRoot.startsWith('..') || isAbsolute(pathFromRoot)) throw new Error('invalid path');
    const info = await stat(path);
    if (!info.isFile()) throw new Error('not a file');
    const data = await readFile(path);
    response.writeHead(200, { 'content-type': types[extname(path)] || 'application/octet-stream' });
    response.end(data);
  } catch {
    response.writeHead(404, { 'content-type': 'text/plain; charset=utf-8' });
    response.end('Not found');
  }
}).listen(port, '127.0.0.1', () => {
  console.log(`Stage Backup dev server: http://127.0.0.1:${port}`);
});
