import assert from "node:assert/strict";
import { test } from "node:test";
import { AuthHeaderClient, credentialsFromEnvironment, credentialsFromProfile, chainCredentials, cacheCredentials } from "./generated-sigv4/src/index.js";
import { signSigV4 } from "./generated-sigv4/src/credentials.js";

test("SigV4 matches the published AWS S3 GET signature", async()=>{
  // https://docs.aws.amazon.com/AmazonS3/latest/developerguide/sig-v4-header-based-auth.html
  const request=new Request("https://examplebucket.s3.amazonaws.com/test.txt",{headers:{range:"bytes=0-9","x-amz-date":"20130524T000000Z"}});
  await signSigV4(request,{accessKeyId:"AKIAIOSFODNN7EXAMPLE",secretAccessKey:"wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"},{region:"us-east-1",name:"s3"});
  assert.ok(request.headers.get("authorization")?.endsWith("Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"));
});

test("generated SigV4 refreshes credentials and signs middleware headers while auth[] bypasses providers", async()=>{
  let identities=0,calls=0;const sent:Request[]=[];
  const client=new AuthHeaderClient({endpoint:"https://example.test",region:"us-east-1",retry:{baseDelayMs:0},awsCredentials:()=>({accessKeyId:`key${++identities}`,secretAccessKey:"secret",sessionToken:`session${identities}`}),middleware:[async(request,next)=>{request.headers.set("x-custom","signed");return next(request);}],fetch:async request=>{sent.push(request);return new Response(null,{status:++calls===1?503:204});}});
  await client.mixed(); assert.equal(identities,2);
  assert.ok(sent[0].headers.get("authorization")?.includes("Credential=key1/"));assert.ok(sent[1].headers.get("authorization")?.includes("Credential=key2/"));
  assert.ok(sent[1].headers.get("authorization")?.includes("x-custom"));assert.equal(sent[1].headers.get("x-amz-security-token"),"session2");
  assert.ok(sent[1].headers.get("authorization")?.includes("/rule-region/overridden/aws4_request"));assert.ok(sent[1].headers.get("authorization")?.includes("x-endpoint"));
  await client.public();assert.equal(identities,2);assert.equal(sent[2].headers.get("authorization"),null);
  await assert.rejects(signSigV4(new Request("https://example.test"),{accessKeyId:"key",secretAccessKey:"secret"},{name:"s3",region:""}),/region/);
  await assert.rejects(signSigV4(new Request("https://example.test",{method:"POST",body:new ReadableStream(),duplex:"half"} as RequestInit),{accessKeyId:"key",secretAccessKey:"secret"},{name:"s3",region:"local"},true),/precomputed/);
});

test("portable profile/environment providers, fallback and coalesced expiration refresh",async()=>{
  assert.equal(credentialsFromEnvironment({}),undefined);assert.throws(()=>credentialsFromEnvironment({AWS_ACCESS_KEY_ID:"partial"}),/incomplete/);
  const profile=credentialsFromProfile("[default]\naws_access_key_id = other\naws_secret_access_key = other\n[profile selected]\naws_access_key_id = selected\naws_secret_access_key = secret\naws_session_token = session","selected");
  assert.equal(profile?.accessKeyId,"selected");assert.equal(profile?.sessionToken,"session");
  assert.equal((await chainCredentials(()=>undefined,profile!)())?.accessKeyId,"selected");
  let calls=0; const provider=cacheCredentials(async()=>{calls++;await Promise.resolve();return {accessKeyId:`key${calls}`,secretAccessKey:"secret",expiration:new Date(Date.now()+3_600_000)};});
  const result=await Promise.all([provider(),provider(),provider()]);assert.equal(calls,1);assert.ok(result.every(value=>value?.accessKeyId==="key1"));
  let refreshes=0;const refresh=cacheCredentials(()=>({accessKeyId:`key${++refreshes}`,secretAccessKey:"secret",expiration:new Date(Date.now()+1000)}));
  await refresh();await refresh();assert.equal(refreshes,2);
  await assert.rejects(cacheCredentials(()=>({accessKeyId:"key",secretAccessKey:"secret",expiration:new Date(0)}))(),/expired/);
});
