import assert from "node:assert/strict";
import { test } from "node:test";
import { mkdtemp, mkdir, writeFile, readFile, rm, symlink } from "node:fs/promises";
import { createHash } from "node:crypto";
import { tmpdir } from "node:os";
import path from "node:path";
import { verifyApiInstallation } from "./windows-api-user-flow-adapter.mjs";

const digest=bytes=>createHash("sha256").update(bytes).digest("hex");
async function fixture() {
  const scratch=await mkdtemp(path.join(tmpdir(),"devbox-suite-delivery-api-test-"));
  const root=path.join(scratch,"Fixture"),assets=path.join(scratch,"assets"),sourceSha="a".repeat(40);
  await mkdir(root);await mkdir(assets);
  const image=Buffer.from("owned synthetic executable bytes");
  const member="generations/g/products/api-studio/devbox-api-studio.exe";
  await mkdir(path.dirname(path.join(root,member)),{recursive:true});await writeFile(path.join(root,member),image);
  const asset=async name=>{const bytes=Buffer.from(`asset ${name}`);await writeFile(path.join(assets,name),bytes);return {name,sha256:digest(bytes)};};
  const products=[];
  for(const id of ["workspace","api-studio","knowledge","control-center"]) products.push({id,portable:await asset(`${id}.zip`),files:[{name:`devbox-${id}.exe`,sha256:digest(image)}]});
  const manifest={schemaVersion:2,sourceSha,products,setup:await asset("setup.exe"),notices:await asset("notices.txt")};
  await writeFile(path.join(assets,"release-manifest.json"),JSON.stringify(manifest));
  await writeFile(path.join(root,"devbox-installation.json"),JSON.stringify({schemaVersion:1,protocolVersion:1,generation:"g",members:[{product:"api-studio",executable:member,sha256:digest(image)}]}));
  await writeFile(path.join(root,"suite-registration.json"),JSON.stringify({schemaVersion:1,installationKey:"b".repeat(64)}));
  await writeFile(path.join(root,"suite-payload.json"),JSON.stringify({sourceSha}));
  return {scratch,root,assets,sourceSha,member};
}
test("installed member uses declared executable digest rather than ZIP digest",async()=>{
  const value=await fixture();
  try {
    const verified=await verifyApiInstallation(value.root,value.assets,value.sourceSha);
    assert.equal(verified.installationKey,"b".repeat(64));
    assert.equal(verified.sourceSha,value.sourceSha);
    assert.equal(Object.keys(verified.artifactDigests).length,7);
    await writeFile(path.join(value.root,value.member),"changed image");
    await assert.rejects(()=>verifyApiInstallation(value.root,value.assets,value.sourceSha));
  } finally {await rm(value.scratch,{recursive:true});}
});
test("installed adapter rejects mismatched source and linked member before launch",async()=>{
  const value=await fixture();
  try {
    await writeFile(path.join(value.root,"suite-payload.json"),JSON.stringify({sourceSha:"c".repeat(40)}));
    await assert.rejects(()=>verifyApiInstallation(value.root,value.assets,value.sourceSha));
    await writeFile(path.join(value.root,"suite-payload.json"),JSON.stringify({sourceSha:value.sourceSha}));
    const image=path.join(value.root,value.member),other=path.join(value.scratch,"other.exe");
    await writeFile(other,await readFile(image));
    await rm(image);await symlink(other,image);
    await assert.rejects(()=>verifyApiInstallation(value.root,value.assets,value.sourceSha));
  } finally {await rm(value.scratch,{recursive:true});}
});
