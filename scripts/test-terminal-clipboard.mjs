import assert from 'node:assert/strict';
import { createServer } from 'vite';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
const cacheDir=await mkdtemp(join(tmpdir(),'noryn-clipboard-'));
const mock=join(cacheDir,'ipc.mjs');
await writeFile(mock,`export const native={calls:[],text:'paste λ',fail:false};export const isTauri=()=>true;export async function invoke(name,args){native.calls.push({name,args});if(native.fail)throw Error('denied');if(name==='clipboard_read_text')return native.text;}`);
const server=await createServer({cacheDir,optimizeDeps:{noDiscovery:true,include:[]},resolve:{alias:{'@tauri-apps/api/core':mock}},root:fileURLToPath(new URL('..',import.meta.url)),server:{middlewareMode:true,hmr:false,watch:null}});
try {
 const {native}=await server.ssrLoadModule(mock);
 const {clipboardKey,copyText,pasteText}=await server.ssrLoadModule('/src/clipboard.ts');
 let errors=0,copied=0,pasted=0,prevented=0;
 const event=(key,extra={})=>({key,type:'keydown',ctrlKey:true,shiftKey:true,metaKey:false,altKey:false,repeat:false,preventDefault(){prevented++},stopPropagation(){},...extra});
 const copy=async()=>{copied++;await copyText('selected ä\ntext')};
 const paste=async()=>{pasted++;assert.equal(await pasteText(),'paste λ')};
 assert.equal(clipboardKey(event('c'),copy,paste,()=>errors++),false);
 await new Promise(r=>setTimeout(r,0));
 assert.equal(native.calls[0].name,'clipboard_write_text');assert.equal(native.calls[0].args.text,'selected ä\ntext');
 assert.equal(clipboardKey(event('c',{type:'keyup'}),copy,paste,()=>errors++),false);assert.equal(copied,1);
 assert.equal(clipboardKey(event('c',{shiftKey:false}),copy,paste,()=>errors++),true,'Ctrl C must still reach SSH as SIGINT');
 assert.equal(clipboardKey(event('v'),copy,paste,()=>errors++),false);await new Promise(r=>setTimeout(r,0));assert.equal(pasted,1);
 assert.equal(clipboardKey(event('c',{ctrlKey:false,metaKey:true,shiftKey:false}),copy,paste,()=>errors++),false);
 await new Promise(r=>setTimeout(r,0));assert.equal(copied,2);
 native.fail=true;clipboardKey(event('c'),copy,paste,()=>errors++);await new Promise(r=>setTimeout(r,0));assert.equal(errors,1,'Clipboard failures must reach the UI');
 assert.equal(prevented,5);console.log('Terminal clipboard: native UTF-8 copy/paste, shortcut phases, macOS chord, SIGINT preservation and visible failures passed.');
} finally {await server.close();await rm(cacheDir,{recursive:true,force:true});}
