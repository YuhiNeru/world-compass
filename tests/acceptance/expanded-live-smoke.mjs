import { chromium } from '@playwright/test';
import assert from 'node:assert/strict';
import { writeFileSync } from 'node:fs';
const browser = await chromium.connectOverCDP('http://127.0.0.1:9223');
try {
  const page = browser.contexts()[0].pages()[0];
  const invoke = (name,args={}) => page.evaluate(([name,args]) => window.__TAURI_INTERNALS__.invoke(name,args),[name,args]);
  assert.equal(await invoke('plugin:app|version'),'0.4.0');
  await page.getByRole('button',{name:'セットアップ',exact:true}).click();
  await page.getByText(/総件数は未確定/).waitFor({timeout:30000});
  await page.getByText(/でログイン済み/).waitFor({timeout:30000});
  const progress=await invoke('corpus_status');
  assert(progress.running && progress.automaticResume && progress.expanded);
  assert(progress.completed>1000 && progress.indexed>1000);
  assert(progress.page>=13);
  assert.equal(progress.category,'チル・睡眠・ホーム・カフェ');
  assert.equal(await page.getByLabel('ワールドDB構築進捗').count(),0);
  assert.equal(await page.getByRole('button',{name:'中断して保存',exact:true}).count(),1);
  await page.screenshot({path:'artifacts/expanded-collection.png'});
  writeFileSync('artifacts/expanded-live-smoke.json',JSON.stringify({version:'0.4.0',automaticRestartVerified:true,expandedProgressVisible:true,initialThousandCapRemoved:true,progress},null,2)+'\n');
  console.log('Expanded collection resumed and UI verified', {worlds:progress.completed,indexed:progress.indexed,page:progress.page});
} finally { await browser.close(); }
