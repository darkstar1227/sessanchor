// Opt-in, read-only remote acceptance. Requires an already configured device.
// Creates local journal sessions/tasks, never installs a remote helper.
const { spawnSync } = require('node:child_process');
const assert = require('node:assert/strict');
const path = require('node:path');
const [state, device] = process.argv.slice(2);
if (!state || !device) throw new Error('Usage: node scripts/verify-windows.cjs STATE_DIR DEVICE_ID');
const binary = process.env.SANC_TEST_BINARY || path.resolve('target/debug/sanc');
const prefix = `verify-${Date.now()}-${process.pid}`;
const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
function cli(args, error) {
  const out = spawnSync(binary, ['--state-dir', state, ...args], { encoding: 'utf8', timeout: 15000 });
  assert.ifError(out.error);
  const value = JSON.parse(out.status === 0 ? out.stdout : out.stderr);
  if (error) { assert.equal(value.error, error); assert.notEqual(out.status, 0); }
  else assert.equal(out.status, 0, JSON.stringify(value));
  return value;
}
function mcp(name, args) {
  const input = [
    {jsonrpc:'2.0', id:1, method:'initialize', params:{protocolVersion:'2025-11-25', capabilities:{}, clientInfo:{name:'acceptance',version:'1'}}},
    {jsonrpc:'2.0', method:'notifications/initialized'},
    {jsonrpc:'2.0', id:2, method:'tools/call', params:{name, arguments:args}},
  ].map(x => JSON.stringify(x)).join('\n') + '\n';
  const out = spawnSync(binary, ['--state-dir',state,'mcp','--allow-exec'], {input,encoding:'utf8',timeout:15000});
  assert.ifError(out.error);
  assert.equal(out.status, 0);
  const response = out.stdout.trim().split('\n').map(JSON.parse).find(x => x.id === 2);
  assert.ok(!response.error, JSON.stringify(response));
  return {value:JSON.parse(response.result.content[0].text), isError:response.result.isError};
}
async function terminal(id) {
  const deadline = Date.now() + 30000;
  while (Date.now() < deadline) {
    const task = cli(['task',String(id)]).task;
    if (['exited','unknown'].includes(task.state)) return task;
    await pause(150);
  }
  throw new Error(`task ${id} timed out; inspect it, do not replay`);
}
function output(id) {
  let cursor = 0, combined = '';
  for (let i = 0; i < 100; i++) {
    const page = cli(['output',String(id),'--cursor',String(cursor)]);
    assert.equal(page.encoding,'utf8');
    assert.ok(Buffer.byteLength(page.output) <= 4096);
    combined += page.output;
    assert.equal(page.next_cursor, cursor + Buffer.byteLength(page.output));
    cursor = page.next_cursor;
    if (!page.more) {
      assert.equal(cli(['output',String(id),'--cursor',String(cursor)]).output,'');
      return combined;
    }
  }
  throw new Error('output pagination failed to finish');
}
async function main() {
  const cases = [
    ['unicode', `Write-Output '中文 😀 " & | $literal'; Write-Output '第二行'`, 0, '中文 😀 " & | $literal\r\n第二行\r\n'],
    ['native', 'cmd /c exit 7', 7, ''],
    ['exit', 'exit 23', 23, ''],
    ['override', 'cmd /c exit 7; exit 0', 0, ''],
    ['throw', 'throw "sanc-readonly-test"', 1, ''],
    ['error', 'Write-Error "sanc-readonly-test"', 1, ''],
    ['missing', 'Get-Item -LiteralPath "Z:\\sanc-nonexistent-acceptance-file"', 1, ''],
    ['parse', 'if (', 1, ''],
    ['pages', `Write-Output ('中😀' * 1600)`, 0, '中😀'.repeat(1600)+'\r\n'],
    ['background', `Write-Output 'start'; Start-Sleep -Seconds 3; Write-Output 'done'`, 0, 'start\r\ndone\r\n'],
  ];
  const failures = [];
  for (const [name, command, exit, expected] of cases) {
    const session = `${prefix}-${name}`;
    cli(['session','create',session,'--device',device]);
    const args = ['exec',session,'--request-id','r','--shell','powershell','--command',command];
    try {
      // Alternate CLI and real stdio MCP submissions; read/retry through CLI.
      const submitted = name === 'unicode' || name === 'error'
        ? mcp('exec',{session,request_id:'r',shell:'powershell',command}).value
        : cli(args);
      const id = submitted.task.task_id;
      if (name === 'background') {
        cli(['exec',session,'--request-id','busy','--shell','powershell','--command','Write-Output never'],'session_busy');
      }
      const task = await terminal(id);
      assert.equal(task.state,'exited');
      assert.equal(task.exit_code,exit);
      assert.equal(output(id),expected);
      assert.equal(cli(args).task.task_id,id);
      cli(['exec',session,'--request-id','r','--command',command],'request_conflict');
      console.log(`PASS ${name} task=${id} exit=${exit}`);
    } catch (e) { failures.push(name); console.log(`FAIL ${name}: ${e.message}`); }
  }
  const blocked = mcp('exec',{session:`${prefix}-unicode`,request_id:'sudo',shell:'powershell',command:' sudo whoami'});
  assert.equal(blocked.isError,true);
  assert.equal(blocked.value.error,'approval_required');
  console.log('PASS MCP leading-sudo hard stop');
  console.log(`RESULT ${cases.length-failures.length}/${cases.length} execution cases; failed=${failures.join(',') || 'none'}`);
  process.exitCode = failures.length ? 1 : 0;
}
main().catch(error => { console.error(error.message); process.exitCode = 1; });
