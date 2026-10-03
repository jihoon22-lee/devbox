import assert from "node:assert/strict";
import { test } from "node:test";
import { connect } from "node:http2";
import { once } from "node:events";
import { echoFixture } from "./windows-api-user-flow-actions.mjs";
import { createOAuthMcpFixture } from "./windows-api-mcp-auth.mjs";
import { createPartialGrpcFixture } from "./windows-api-grpc.mjs";

test("owned HTTP fixture captures exact encoded request and closes",async()=>{
  const fixture=await echoFixture();
  try {
    const body="dup=%20a%26%3D%23%ED%95%9C%EA%B8%80%20&dup=";
    await fetch(`${fixture.url}/echo?dup=1&dup=`,{method:"POST",body});
    assert.equal(fixture.hits[0].body,body);
    assert.equal(fixture.hits[0].url,"/echo?dup=1&dup=");
  } finally {await fixture.close();}
  await assert.rejects(()=>fetch(fixture.url));
});
test("owned MCP fixture distinguishes call bearer and revocation",async()=>{
  const fixture=await createOAuthMcpFixture();
  try {
    const response=await fetch(fixture.endpoint,{method:"POST",headers:{Authorization:"Bearer synthetic-A"},body:JSON.stringify({jsonrpc:"2.0",id:"one",method:"tools/list",params:{}})});
    assert.equal((await response.json()).result.tools[0].name,"synthetic_echo");
    await fetch(`${fixture.base}/revoke`,{method:"POST",body:"token=synthetic-A"});
    assert.equal(fixture.calls[0].authorization,"Bearer synthetic-A");
    assert.equal(fixture.revocations[0].token,"synthetic-A");
  } finally {await fixture.close();}
});
test("owned HTTP2 fixture sends two frames before INTERNAL trailers",async()=>{
  const fixture=await createPartialGrpcFixture();
  const client=connect(fixture.endpoint);
  try {
    for(const method of ["Server","Bidi"]) {
      const stream=client.request({":method":"POST",":path":`/fixture.Partial/${method}`,"content-type":"application/grpc"});
      let bytes=Buffer.alloc(0),status;
      stream.on("data",chunk=>{bytes=Buffer.concat([bytes,chunk]);});
      stream.on("trailers",headers=>{status=headers["grpc-status"];});
      stream.end(Buffer.alloc(5));await once(stream,"end");
      const frames=[];
      while(bytes.length>=5){const length=bytes.readUInt32BE(1);frames.push(bytes.subarray(5,5+length).toString("utf8"));bytes=bytes.subarray(5+length);}
      assert.equal(status,"13");assert.equal(frames.length,2);
      assert.ok(frames[0].includes("first-synthetic")&&frames[1].includes("second-synthetic"));
    }
  } finally {client.close();await fixture.close();}
});
