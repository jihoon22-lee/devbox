import test from 'node:test';
import assert from 'node:assert/strict';
import {assertProductLayout} from './browser-product-layout.mjs';
test('rejects collapsed editor, clipped controls and notice overlap; not a DOM layout simulation',()=>{
 const measured={main:{width:468,y:150},editor:{width:468},notice:{y:80,height:50},horizontalOverflow:false,clippedPrimaryControls:[]};
 assert.equal(assertProductLayout(measured,{editor:true}),measured);
 for(const failed of [{...measured,editor:{width:33}},{...measured,notice:{y:80,height:100}},{...measured,clippedPrimaryControls:['Save']}])assert.throws(()=>assertProductLayout(failed,{editor:true}));
});
