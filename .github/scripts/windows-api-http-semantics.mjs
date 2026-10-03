import assert from "node:assert/strict";
import { setTimeout as delay } from "node:timers/promises";
import { requireApiContext, button, textbox, select, scenario, echoFixture, until, expectText, bodyText } from "./windows-api-user-flow-actions.mjs";

export const SCENARIO_IDS = Object.freeze(["HTTP-01", "HTTP-02", "HTTP-03"]);

export async function run(context) {
  requireApiContext(context);
  const fixture = await echoFixture();
  const results=[];
  const send=async()=>{
    const count=fixture.hits.length;
    const start=performance.now();
    await context.ui.click(button("보내기"));
    await until(()=>fixture.hits.length===count+1,"HTTP fixture did not receive request");
    await expectText(context,"요청이 완료되었습니다.");
    context.httpCompletedMs=Math.max(context.httpCompletedMs??0,performance.now()-start);
    return fixture.hits.at(-1);
  };
  try {
    await context.ui.click(button("요청"));
    results.push(await scenario(context,"HTTP-01",async record=>{
      await select(context,"HTTP method",1);
      await context.ui.fill(textbox("요청 URL"),`${fixture.url}/body`);
      await context.ui.click(button("BODY"));
      await select(context,"요청 본문 형식",1);
      await context.ui.fill(textbox("요청 본문"),"{{missing-inactive-body}}");
      await select(context,"요청 본문 형식",0);
      assert.equal((await send()).body,"");
      record("Visible JSON→None switch sends an empty native body despite inactive unresolved template");
      await select(context,"요청 본문 형식",4);
      await context.ui.fill(textbox("요청 본문"),"synthetic raw 본문");
      assert.equal((await send()).body,"synthetic raw 본문");
      record("Visible raw body produces exact UTF-8 native request bytes");
      await context.ui.click(button("AUTH"));
      await select(context,"인증 종류",1);
      await context.ui.fill(textbox("사용자 이름"),"{{missing-inactive-user}}");
      await context.ui.fill(textbox("비밀번호"),"{{missing-inactive-password}}");
      await select(context,"인증 종류",2);
      await context.ui.fill(textbox("토큰"),"fixture-bearer");
      assert.equal((await send()).headers.authorization,"Bearer fixture-bearer");
      await select(context,"인증 종류",0);
      assert.equal((await send()).headers.authorization,undefined);
      record("Basic→Bearer→None switches resolve only active native authentication fields");
    }));
    if(results.at(-1).status!=="PASS") return results;
    results.push(await scenario(context,"HTTP-02",async record=>{
      await context.ui.fill(textbox("요청 URL"),`${fixture.url}/encoded?first=1#fragment`);
      await context.ui.click(button("PARAMS"));
      const pairs=[["dup"," a&=#한글 "],["dup",""],["emoji","🙂"]];
      for(let i=0;i<pairs.length;i++) {
        await context.ui.click(button("+ 추가"));
        await context.ui.fill(textbox(`쿼리 ${i+1} 이름`),pairs[i][0]);
        await context.ui.fill(textbox(`쿼리 ${i+1} 값`),pairs[i][1]);
      }
      await context.ui.click(button("BODY"));
      await select(context,"요청 본문 형식",2);
      await context.ui.fill(textbox("요청 본문"),"dup= a&=#한글 \ndup=\nemoji=🙂");
      const hit=await send();
      assert.deepEqual([...new URL(hit.url,fixture.url).searchParams],[['first','1'],...pairs]);
      assert.deepEqual([...new URLSearchParams(hit.body)],pairs);
      assert.ok(!hit.url.includes("#"));
      record("Actual native echo preserves duplicate ordering, empty values, whitespace, Unicode and reserved query/form characters before the URL fragment");
      await context.ui.click(button("cURL"));
      await expectText(context,"--data");
      const curl=await context.cdp.evaluate("document.querySelector('.curl-text').textContent");
      assert.ok(curl.includes(new URLSearchParams(pairs).toString()));
      record("Visible generated cURL contains exactly encoded form bytes matching the native echo");
      await context.ui.click(button("코드"));
      const text=await bodyText(context);
      assert.ok(text.includes("%") && text.includes("dup"));
      record("User-visible generated code includes percent-encoded duplicate form/query fields");
    }));
    if(results.at(-1).status!=="PASS") return results;
    results.push(await scenario(context,"HTTP-03",async record=>{
      await context.ui.click(button("BODY"));
      await select(context,"요청 본문 형식",0);
      await context.ui.fill(textbox("요청 URL"),`${fixture.url}/completed`);
      const first=await send();
      assert.equal(first.body,"");
      await expectText(context,`HTTP A response ${fixture.hits.length}`);
      await context.ui.fill(textbox("요청 URL"),`${fixture.url}/delayed`);
      await context.ui.click(button("보내기"));
      await expectText(context,"이전 요청의 응답입니다");
      await context.ui.click(button("취소"));
      await expectText(context,"서버 작업의 취소 여부는 확인이 필요합니다");
      await delay(1800);
      assert.ok(!(await bodyText(context)).includes("HTTP B late response"));
      record("Real delayed B cancellation preserves labelled response A and ignores late B body/status");
    }));
    return results;
  } finally { await fixture.close(); }
}
