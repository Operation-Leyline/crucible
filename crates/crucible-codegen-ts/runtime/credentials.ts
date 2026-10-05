import { AuthenticationError, type IdentityProvider } from "./runtime.js";

export interface AwsCredentials {
  readonly accessKeyId: string;
  readonly secretAccessKey: string;
  readonly sessionToken?: string;
  readonly expiration?: Date;
}

export function credentialsFromEnvironment(environment: Readonly<Record<string,string | undefined>>): AwsCredentials | undefined {
  const accessKeyId=environment.AWS_ACCESS_KEY_ID, secretAccessKey=environment.AWS_SECRET_ACCESS_KEY;
  if (!accessKeyId && !secretAccessKey) return undefined;
  if (!accessKeyId || !secretAccessKey) throw new AuthenticationError("incomplete AWS environment credentials");
  return { accessKeyId,secretAccessKey,sessionToken:environment.AWS_SESSION_TOKEN };
}

/** Parse caller-supplied shared credentials/config text; no Node-only file API. */
export function credentialsFromProfile(text: string, profile="default"): AwsCredentials | undefined {
  let selected=false; const values: Record<string,string> = Object.create(null);
  for (const raw of text.split(/\r?\n/)) {
    const line=raw.trim(); if (!line || /^[#;]/.test(line)) continue;
    if (line.startsWith("[") && line.endsWith("]")) { const name=line.slice(1,-1).trim().replace(/^profile\s+/,""); selected=name===profile; continue; }
    if (!selected) continue;
    const index=line.indexOf("="); if (index<0) continue; values[line.slice(0,index).trim()]=line.slice(index+1).trim();
  }
  const accessKeyId=values.aws_access_key_id,secretAccessKey=values.aws_secret_access_key;
  if (!accessKeyId && !secretAccessKey) return undefined;
  if (!accessKeyId || !secretAccessKey) throw new AuthenticationError("incomplete AWS profile credentials");
  return {accessKeyId,secretAccessKey,sessionToken:values.aws_session_token};
}

export function chainCredentials(...providers: readonly IdentityProvider<AwsCredentials>[]): () => Promise<AwsCredentials | undefined> {
  return async () => { for (const provider of providers) { const value=await (typeof provider==="function"?provider():provider); if (value) return value; } return undefined; };
}

/** Coalesces concurrent refreshes and refreshes before the identity expires. */
export function cacheCredentials(provider: IdentityProvider<AwsCredentials>, refreshBeforeMs=300_000): () => Promise<AwsCredentials | undefined> {
  if (!Number.isFinite(refreshBeforeMs) || refreshBeforeMs<0) throw new TypeError("credential refresh window must be finite and nonnegative");
  let cached: AwsCredentials | undefined, pending: Promise<AwsCredentials | undefined> | undefined;
  return async () => {
    if (cached && (!cached.expiration || cached.expiration.getTime()-Date.now()>refreshBeforeMs)) return cached;
    if (!pending) pending=Promise.resolve().then(()=>typeof provider==="function"?provider():provider).then(value=>{
      if (value?.expiration && (!Number.isFinite(value.expiration.getTime()) || value.expiration.getTime()<=Date.now())) throw new AuthenticationError("AWS credentials are expired");
      cached=value; return value;
    }).finally(()=>{pending=undefined;});
    return pending;
  };
}

const bytes=(value:string)=>new TextEncoder().encode(value);
const hex=(value:ArrayBuffer)=>Array.from(new Uint8Array(value),b=>b.toString(16).padStart(2,"0")).join("");
const hash=async(value:BufferSource)=>hex(await crypto.subtle.digest("SHA-256",value));
const encode=(value:string)=>encodeURIComponent(value).replace(/[!'()*]/g,c=>`%${c.charCodeAt(0).toString(16).toUpperCase()}`);
async function hmac(key: BufferSource, value:string): Promise<ArrayBuffer> {
  const imported=await crypto.subtle.importKey("raw",key,{name:"HMAC",hash:"SHA-256"},false,["sign"]);
  return crypto.subtle.sign("HMAC",imported,bytes(value));
}
export interface SigningScope { readonly region: string; readonly name: string; readonly disableDoubleEncoding?: boolean; readonly disableNormalizePath?: boolean; readonly unsignedPayload?: boolean; }

export async function signSigV4(request: Request, credentials: AwsCredentials, scope: SigningScope, streaming=false): Promise<Request> {
  if (!credentials.accessKeyId || !credentials.secretAccessKey || !scope.region || !scope.name) throw new AuthenticationError("SigV4 requires credentials, region, and signing name");
  if (credentials.expiration && (!Number.isFinite(credentials.expiration.getTime()) || credentials.expiration.getTime()<=Date.now())) throw new AuthenticationError("AWS credentials are expired");
  if (/[\s/,]/.test(credentials.accessKeyId+scope.region+scope.name)) throw new AuthenticationError("invalid SigV4 credential scope");
  const date=request.headers.get("x-amz-date") ?? new Date().toISOString().replace(/[:-]|\.\d{3}/g,"");
  if (!/^\d{8}T\d{6}Z$/.test(date)) throw new AuthenticationError("invalid x-amz-date");
  request.headers.set("x-amz-date",date);
  if (credentials.sessionToken) request.headers.set("x-amz-security-token",credentials.sessionToken);
  let payload=request.headers.get("x-amz-content-sha256");
  if (!payload) {
    if (scope.unsignedPayload) payload="UNSIGNED-PAYLOAD";
    else if (streaming) throw new AuthenticationError("SigV4 streaming payload requires a precomputed ByteStream sha256");
    else payload=await hash(request.body ? await request.clone().arrayBuffer() : new ArrayBuffer(0));
    request.headers.set("x-amz-content-sha256",payload);
  }
  const url=new URL(request.url); let path=url.pathname;
  if (!(scope.disableNormalizePath ?? scope.name==="s3")) path=path.replace(/\/{2,}/g,"/");
  if (!(scope.disableDoubleEncoding ?? scope.name==="s3")) path=path.split("/").map(encode).join("/");
  const pairs:[string,string][]=[];url.searchParams.forEach((value,name)=>pairs.push([encode(name),encode(value)]));
  const query=pairs.sort(([an,av],[bn,bv])=>an<bn?-1:an>bn?1:av<bv?-1:av>bv?1:0).map(([name,value])=>`${name}=${value}`).join("&");
  const excluded=new Set(["authorization","connection","expect","transfer-encoding","user-agent","x-amzn-trace-id","accept","accept-encoding","content-length"]);
  const headers=new Map<string,string>(); request.headers.forEach((value,name)=>{if (!excluded.has(name)) headers.set(name,value.trim().replace(/\s+/g," "));});headers.set("host",url.host);
  const names=Array.from(headers.keys()).sort(), signed=names.join(";");
  const canonical=[request.method,path||"/",query,names.map(name=>`${name}:${headers.get(name)}\n`).join(""),signed,payload].join("\n");
  const credentialScope=`${date.slice(0,8)}/${scope.region}/${scope.name}/aws4_request`;
  const toSign=`AWS4-HMAC-SHA256\n${date}\n${credentialScope}\n${await hash(bytes(canonical))}`;
  let key=await hmac(bytes(`AWS4${credentials.secretAccessKey}`),date.slice(0,8));key=await hmac(key,scope.region);key=await hmac(key,scope.name);key=await hmac(key,"aws4_request");
  request.signal.throwIfAborted();
  request.headers.set("authorization",`AWS4-HMAC-SHA256 Credential=${credentials.accessKeyId}/${credentialScope}, SignedHeaders=${signed}, Signature=${hex(await hmac(key,toSign))}`);
  return request;
}
