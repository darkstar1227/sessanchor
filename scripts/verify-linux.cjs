// Opt-in Linux remote acceptance. No remote files, installs or privilege changes.
const {spawnSync} = require('node:child_process');
const assert = require('node:assert/strict');
const path = require('node:path');
const [state, device] = process.argv.slice(2);
if (!state || !device) throw new Error('Usage: node scripts/verify-linux.cjs STATE_DIR DEVICE_ID');
const binary = process.env.SANC_TEST_BINARY || path.resolve('target/debug/sanc');
const prefix = `linux-${Date.now()}-${process.pid}`;
function cli(args, error) {
  const out = spawnSync(binary, ['--state-dir',state,...args], {encoding:'utf8',timeout:20000});
  assert.ifError(out.error);
  const value = JSON.parse(out.status === 0 ? out.stdout : out.stderr);
  if (error) { assert.notEqual(out.status,0); assert.equal(value.error,error); }
  else assert.equal(out.status,0,JSON.stringify(value));
  return value;
}
function mcp(args) {
  const input = [
    {jsonrpc:'2.0',id:1,method:'initialize',params:{protocolVersion:'2025-11-25',capabilities:{},clientInfo:{name:'linux-acceptance',version:'1'}}},
    {jsonrpc:'2.0',method:'notifications/initialized'},
    {jsonrpc:'2.0',id:2,method:'tools/call',params:{name:'exec',arguments:args}},
  ].map(JSON.stringify).join('\n')+'\n';
  const out = spawnSync(binary,['--state-dir',state,'mcp','--allow-exec'],{input,encoding:'utf8',timeout:20000});
  assert.ifError(out.error);
  assert.equal(out.status,0);
  const response = out.stdout.trim().split('\n').map(JSON.parse).find(x=>x.id===2);
  assert.ok(!response.error,JSON.stringify(response));
  return {error:response.result.isError,value:JSON.parse(response.result.content[0].text)};
}
async function wait(id) {
  const deadline = Date.now()+30000;
  while(Date.now()<deadline) {
    const task=cli(['task',String(id)]).task;
    if(['exited','unknown'].includes(task.state)) return task;
    await new Promise(resolve=>setTimeout(resolve,150));
  }
  throw new Error(`Timeout task=${id}; inspect, never replay`);
}
function output(id,stream='stdout') {
  let cursor=0;
  const buffers=[];
  for(let i=0;i<100;i++) {
    const page=cli(['output',String(id),'--stream',stream,'--cursor',String(cursor)]);
    assert.ok(Buffer.byteLength(page.output)<=4096);
    assert.ok(['utf8','hex'].includes(page.encoding));
    const bytes=Buffer.from(page.output,page.encoding);
    assert.equal(page.next_cursor,cursor+bytes.length);
    cursor=page.next_cursor;
    buffers.push(bytes);
    if(!page.more) {
      assert.equal(cli(['output',String(id),'--stream',stream,'--cursor',String(cursor)]).output,'');
      return Buffer.concat(buffers);
    }
  }
  throw new Error('Pagination did not finish');
}
async function main() {
  cli(['device','probe',device]);
  cli(['device','inspect',device,'--platform','posix']);
  const cases=[
    ['unicode', `printf '%s\\n' '中文 😀 " & | $literal'`,0,'中文 😀 " & | $literal\n',''],
    ['exit', 'exit 7',7,'',''],
    ['stderr', `printf 'out\\n'; printf 'err\\n' >&2`,0,'out\n','err\n'],
    ['pages', `i=0; while [ "$i" -lt 1600 ]; do printf '中😀'; i=$((i+1)); done`,0,'中😀'.repeat(1600),''],
    ['binary', `printf '\\377\\000\\376'`,0,Buffer.from([255,0,254]),''],
    ['background', `printf 'start\\n'; sleep 3; printf 'done\\n'`,0,'start\ndone\n',''],
    ['unknown', 'exit 255',null,'',''],
  ];
  let passed=0;
  for(const [name,command,exit,stdout,stderr] of cases) {
    const session=`${prefix}-${name}`;
    cli(['session','create',session,'--device',device]);
    const args=['exec',session,'--request-id','r','--command',command];
    try {
      const submitted=name==='unicode'?mcp({session,request_id:'r',command}).value:cli(args);
      const id=submitted.task.task_id;
      assert.equal(cli(args).task.task_id,id);
      if(name==='background') cli(['exec',session,'--request-id','other','--command','true'],'session_busy');
      const task=await wait(id);
      assert.equal(task.state,exit===null?'unknown':'exited');
      assert.equal(task.exit_code,exit);
      assert.deepEqual(output(id),Buffer.from(stdout));
      assert.deepEqual(output(id,'stderr'),Buffer.from(stderr));
      assert.equal(cli(args).task.task_id,id);
      cli(['exec',session,'--request-id','r','--command','true'],'request_conflict');
      if(exit===null) cli(['exec',session,'--request-id','other','--command','true'],'session_busy');
      passed++;
      console.log(`PASS ${name} task=${id} state=${task.state} exit=${exit}`);
    } catch(e) { console.log(`FAIL ${name}: ${e.message}`); }
  }
  const blocked=mcp({session:`${prefix}-unicode`,request_id:'sudo',command:' sudo id'});
  assert.equal(blocked.error,true);
  assert.equal(blocked.value.error,'approval_required');
  console.log('PASS MCP sudo hard stop');
  console.log(`RESULT ${passed}/${cases.length} execution cases`);
  process.exitCode=passed===cases.length?0:1;
}
main().catch(e=>{console.error(e.message);process.exitCode=1;});
