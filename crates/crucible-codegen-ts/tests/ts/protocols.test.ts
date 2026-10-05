import assert from "node:assert/strict";
import { test } from "node:test";
import { WireClient,TextError } from "./generated-payloads/src/index.js";
import * as rest from "./generated-restJson1/src/index.js";
import * as json10 from "./generated-awsJson1_0/src/index.js";
import * as json11 from "./generated-awsJson1_1/src/index.js";

test("string and document payloads, encoded media headers and raw modeled errors",async()=>{
  const client=new WireClient({endpoint:"https://example.test",fetch:async request=>{
    if (new URL(request.url).pathname==="/document") {assert.equal(await request.text(),'{"name":"record"}');return new Response('{"name":"response"}');}
    assert.equal(await request.text(),"raw text");assert.equal(request.headers.get("content-type"),"text/plain");assert.equal(request.headers.get("x-meta"),"eyJhIjoxfQ==, w6k=");assert.equal(request.headers.get("x-extra-one"),"aGVsbG8=");
    return new Response("raw response",{headers:{"x-meta":"eyJhIjoxfQ==, w6k=","x-extra-one":"aGVsbG8="}});
  }});
  const output=await client.text({text:"raw text",meta:['{"a":1}',"é"],extras:{one:"hello"}});assert.equal(output.text,"raw response");assert.deepEqual(output.meta,['{"a":1}',"é"]);assert.deepEqual(output.extras,{one:"hello"});
  assert.deepEqual((await client.document({document:{name:"record"}})).document,{name:"response"});
  const failed=new WireClient({endpoint:"https://example.test",fetch:async()=>new Response("invalid text",{status:422,headers:{"x-crucible-error":"TextError"}})});
  try {await failed.text({text:"bad"});throw new Error("expected modeled raw error");}catch(error){assert.ok(error instanceof TextError);assert.equal((error as TextError).details,"invalid text");}
});

for (const [sdk,version] of [[rest,undefined],[json10,"1.0"],[json11,"1.1"]] as const) {
  test(`AWS JSON ${version ?? "REST"} uses protocol body formats and HTTP binding overrides`,async()=>{
    const instant=new Date(-250);let calls=0;
    const client=new sdk.JsonServiceClient({endpoint:"https://example.test",fetch:async request=>{
      calls++; const body=JSON.parse(await request.text());assert.equal(body.when,-0.25);assert.equal(body.iso,"1969-12-31T23:59:59.750Z");assert.equal(body.floatValue,"NaN");assert.equal(body.doubleValue,"Infinity");assert.equal(body.document,null);
      assert.equal(body[version?"renamed":"wire-name"],"hello");
      const headers:Record<string,string>={};
      if(version){assert.equal(new URL(request.url).pathname,"/");assert.equal(request.headers.get("x-amz-target"),"JsonService.Echo");assert.equal(request.headers.get("content-type"),`application/x-amz-json-${version}`);assert.equal(body.date,-0.25);assert.deepEqual(body.dates,[-0.25]);}
      else {assert.equal(new URL(request.url).pathname,"/echo");assert.equal(new URL(request.url).searchParams.get("time"),instant.toISOString());assert.equal(request.headers.get("x-date"),"Wed, 31 Dec 1969 23:59:59 GMT");assert.equal(request.headers.get("x-dates"),"Wed, 31 Dec 1969 23:59:59 GMT");headers["x-dates"]="Wed, 31 Dec 1969 23:59:59 GMT, Thu, 01 Jan 1970 00:00:00 GMT";}
      body.choice={text:"ok",__type:"ignored"};body.nullable=null;return new Response(JSON.stringify(body),{headers});
    }});
    const output=await client.echo({renamed:"hello",when:instant,iso:instant,date:instant,dates:[instant],query:instant,document:null,floatValue:NaN,doubleValue:Infinity});
    assert.equal(output.when.getTime(),-250);assert.equal(output.iso.getTime(),-250);assert.deepEqual(output.choice,{text:"ok"});assert.equal(output.nullable,undefined);assert.equal(output.document,null);assert.ok(Number.isNaN(output.floatValue));assert.equal(output.doubleValue,Infinity);if(!version)assert.deepEqual(output.dates?.map(value=>value.getTime()),[-1000,0]);assert.equal(calls,1);
  });
  test(`AWS JSON ${version ?? "REST"} identifies modeled errors and sends empty RPC bodies`,async()=>{
    for(const discriminator of [{code:"example.json#Problem"},{__type:"example.json#Problem"}]) {
      const client=new sdk.JsonServiceClient({endpoint:"https://example.test",fetch:async()=>new Response(JSON.stringify({...discriminator,Message:"bad"}),{status:409,headers:{"x-amzn-requestid":"request-42"}})});
      try {await client.echo({});throw new Error("expected modeled error");}catch(error){assert.ok(error instanceof sdk.Problem);assert.equal((error as rest.Problem).message,"bad");assert.equal((error as rest.Problem).$metadata.requestId,"request-42");}
    }
    const client=new sdk.JsonServiceClient({endpoint:"https://example.test",fetch:async request=>{assert.equal(await request.text(),version?"{}":"");if(version)assert.equal(request.headers.get("x-amz-target"),"JsonService.Empty");return new Response(null,{status:200});}});
    await client.empty();
  });
}
