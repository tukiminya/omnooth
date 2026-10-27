import { useEffect, useMemo, useRef, useState } from 'react';
import type { Catalog, ReviewState } from '../types';
import { createDatabaseModel, databaseViews, type DatabaseModel } from './database-model';
import './database-diagram.css';

const CARD_WIDTH=284, COLUMN_GAP=112, ROW_GAP=116, PADDING=54;
type DiagramLayout = {
  nodes: {table: DatabaseModel['tables'][number];x:number;y:number;width:number;height:number;row:number;col:number;rowBottom:number;fields:DatabaseModel['tables'][number]['fields']}[];
  width:number;height:number;
};
function layoutTables(model:DatabaseModel,rows:string[][],allFields:boolean):DiagramLayout {
  const nodes:DiagramLayout['nodes']=[];
  let y=PADDING;
  for (const [row,names] of rows.entries()) {
    const rowNodes=names.flatMap((name,col)=>{
      const table=model.tables.find(t=>t.name===name);
      if(!table)return [];
      const fields=allFields?table.fields:table.fields.filter(f=>f.primary||f.foreign||['name','version_label','title'].includes(f.name));
      return [{table,x:PADDING+col*(CARD_WIDTH+COLUMN_GAP),y,width:CARD_WIDTH,height:86+fields.length*32+42,row,col,rowBottom:0,fields}];
    });
    const rowHeight=Math.max(0,...rowNodes.map(n=>n.height));
    nodes.push(...rowNodes.map(n=>({...n,rowBottom:y+rowHeight})));
    y+=rowHeight+ROW_GAP;
  }
  return {nodes,width:PADDING*2+Math.max(...rows.map(r=>r.length))*(CARD_WIDTH+COLUMN_GAP)-COLUMN_GAP,height:y-ROW_GAP+PADDING+40};
}
function relationshipPath(parent:DiagramLayout['nodes'][number],child:DiagramLayout['nodes'][number],lane:number) {
  const px=parent.x,py=parent.y,cx=child.x,cy=child.y;
  if(parent.table.name===child.table.name) {
    const x=px+CARD_WIDTH,y=py+46,outer=x+48;
    return {points:[[x,y],[outer,y],[outer,py+parent.height+54],[px+CARD_WIDTH/2,py+parent.height+54],[px+CARD_WIDTH/2,py+parent.height]],start:[x+18,y-17],end:[px+CARD_WIDTH/2+34,py+parent.height+21]};
  }
  if(parent.row===child.row&&Math.abs(parent.col-child.col)===1) {
    const right=cx>px;
    const x1=right?px+CARD_WIDTH:px,x2=right?cx:cx+CARD_WIDTH;
    // 役割の違う二重参照（owner / required）を同じ線に重ねない。
    const y=py+35+lane*35;
    return {points:[[x1,y],[x2,y]],start:[x1+(right?22:-22),y-17],end:[x2+(right?-29:29),y-17]};
  }
  if(parent.row===child.row) {
    const y=py-30-lane*18;
    return {points:[[px+CARD_WIDTH/2,py],[px+CARD_WIDTH/2,y],[cx+CARD_WIDTH/2,y],[cx+CARD_WIDTH/2,cy]],start:[px+CARD_WIDTH/2-30,py-13],end:[cx+CARD_WIDTH/2+34,cy-13]};
  }
  const down=cy>py;
  const startY=down?py+parent.height:py;
  const endY=down?cy:cy+child.height;
  const firstRail=down?parent.rowBottom+34+lane*12:py-34-lane*12;
  const lastRail=down?cy-34-lane*12:child.rowBottom+34+lane*12;
  const corridor=px+CARD_WIDTH+Math.min(42+lane*8,50);
  return {
    points:[[px+CARD_WIDTH/2,startY],[px+CARD_WIDTH/2,firstRail],[corridor,firstRail],[corridor,lastRail],[cx+CARD_WIDTH/2,lastRail],[cx+CARD_WIDTH/2,endY]],
    start:[px+CARD_WIDTH/2-30,startY+(down?23:-14)],end:[cx+CARD_WIDTH/2+34,endY+(down?-14:23)],
  };
}
function pathString(points:number[][]) {return points.map(([x,y],i)=>(i?'L':'M')+x+','+y).join(' ');}

export default function DatabaseDiagram({catalog,reviews,open}:{catalog:Catalog;reviews:ReviewState;open:(id:string)=>void}) {
  const model=useMemo(()=>createDatabaseModel(catalog.specs),[catalog]);
  const [viewId,setViewId]=useState('library');
  const [selected,setSelected]=useState<string|null>(null);
  const [selectedRelation,setSelectedRelation]=useState<string|null>(null);
  const [scrollTarget,setScrollTarget]=useState<string|null>(null);
  const [allFields,setAllFields]=useState(false);
  const [automatic,setAutomatic]=useState(true);
  const [manualZoom,setManualZoom]=useState(1);
  const [viewportWidth,setViewportWidth]=useState(1000);
  const viewport=useRef<HTMLDivElement>(null);
  const drag=useRef<{x:number;y:number;left:number;top:number}|null>(null);
  const view=databaseViews.find(v=>v.id===viewId)!;
  const layout=useMemo(()=>layoutTables(model,view.rows,allFields),[model,view,allFields]);
  const tableNames=new Set(layout.nodes.map(n=>n.table.name));
  const visibleRelations=model.relations.filter(r=>tableNames.has(r.parent)&&tableNames.has(r.child));
  const zoom=automatic?Math.max(.65,Math.min(1,Math.floor((viewportWidth-20)/layout.width*20)/20)):manualZoom;
  const picked=model.tables.find(t=>t.name===selected);
  const related=model.relations.filter(r=>r.parent===selected||r.child===selected);
  const focusedNames=new Set(selected?[selected,...related.flatMap(r=>[r.parent,r.child])]:model.tables.map(t=>t.name));
  useEffect(()=>{
    if(!viewport.current)return;
    const observer=new ResizeObserver(entries=>setViewportWidth(entries[0].contentRect.width));
    observer.observe(viewport.current);
    return ()=>observer.disconnect();
  },[]);
  useEffect(()=>{
    if(!scrollTarget||!viewport.current)return;
    const node=layout.nodes.find(n=>n.table.name===scrollTarget);
    if(!node)return;
    viewport.current.scrollTo({left:Math.max(0,node.x*zoom-30),top:Math.max(0,node.y*zoom-30)});
    setScrollTarget(null);
  },[layout,scrollTarget,zoom]);
  const changeView=(id:string)=>{setViewId(id);setSelected(null);setSelectedRelation(null);setAutomatic(true);viewport.current?.scrollTo(0,0);};
  const resize=(amount:number)=>{setManualZoom(Math.max(.5,Math.min(1.5,Math.round((zoom+amount)*100)/100)));setAutomatic(false);};
  const selectTable=(name:string)=>{setSelected(name===selected?null:name);setSelectedRelation(null);};
  const inspectRelation=(relation:DatabaseModel['relations'][number])=>{
    if(!tableNames.has(relation.parent)||!tableNames.has(relation.child)) {
      setViewId('all');setAutomatic(true);setScrollTarget(relation.child);
    }
    setSelected(relation.child);setSelectedRelation(relation.id);
  };
  return <section className="database-er" aria-label="データベースの ER 図">
    <div className="er-view-tabs" role="tablist" aria-label="ER 図の表示範囲">{databaseViews.map(v=><button role="tab" aria-selected={viewId===v.id} key={v.id} onClick={()=>changeView(v.id)}>{v.title}</button>)}</div>
    <div className="er-toolbar">
      <label><input type="checkbox" checked={allFields} onChange={e=>setAllFields(e.target.checked)}/>すべての列</label>
      <div className="er-zoom">
        <button onClick={()=>resize(-.1)} disabled={zoom<=.5} aria-label="ER 図を縮小">−</button>
        <output aria-live="polite">{Math.round(zoom*100)}%</output>
        <button onClick={()=>resize(.1)} disabled={zoom>=1.5} aria-label="ER 図を拡大">＋</button>
        <button onClick={()=>{setAutomatic(true);viewport.current?.scrollTo(0,0);}}>幅に合わせる</button>
        <button onClick={()=>{setAutomatic(false);setManualZoom(1);viewport.current?.scrollTo(0,0);}}>100%</button>
      </div>
    </div>
    <div className="er-viewport" ref={viewport} tabIndex={0} aria-label="ER 図"
      style={{height:Math.max(440,Math.min(680,layout.height*zoom+8))}}
      onPointerDown={e=>{if((e.target as Element).closest('button,[role="button"]'))return;const el=e.currentTarget;drag.current={x:e.clientX,y:e.clientY,left:el.scrollLeft,top:el.scrollTop};el.setPointerCapture(e.pointerId);}}
      onPointerMove={e=>{if(!drag.current)return;e.currentTarget.scrollLeft=drag.current.left-(e.clientX-drag.current.x);e.currentTarget.scrollTop=drag.current.top-(e.clientY-drag.current.y);}}
      onPointerUp={()=>{drag.current=null;}} onPointerCancel={()=>{drag.current=null;}}>
      <div className="er-scroll-size" style={{width:layout.width*zoom,height:layout.height*zoom}}>
        <div className="er-canvas" style={{width:layout.width,height:layout.height,transform:'scale('+zoom+')'}}>
          <svg className="er-lines" width={layout.width} height={layout.height} aria-label="外部キーの関連線">
            <defs>
              <marker id="er-one" viewBox="0 -8 16 16" refX="0" refY="0" markerWidth="16" markerHeight="16" markerUnits="userSpaceOnUse" orient="auto-start-reverse"><path d="M 8 -7 V 7 M 14 -7 V 7" fill="none" stroke="context-stroke" strokeWidth="1.7"/></marker>
              <marker id="er-optional" viewBox="0 -8 25 16" refX="0" refY="0" markerWidth="25" markerHeight="16" markerUnits="userSpaceOnUse" orient="auto-start-reverse"><path d="M 8 -7 V 7" fill="none" stroke="context-stroke" strokeWidth="1.7"/><circle cx="19" cy="0" r="4" fill="white" stroke="context-stroke" strokeWidth="1.7"/></marker>
              <marker id="er-many" viewBox="-27 -9 28 18" refX="0" refY="0" markerWidth="28" markerHeight="18" markerUnits="userSpaceOnUse" orient="auto"><path d="M 0 -8 L -13 0 L 0 8 M -13 0 H 0" fill="none" stroke="context-stroke" strokeWidth="1.7"/><circle cx="-21" cy="0" r="4" fill="white" stroke="context-stroke" strokeWidth="1.7"/></marker>
            </defs>
            {visibleRelations.map(relation=>{
              const parent=layout.nodes.find(n=>n.table.name===relation.parent)!;
              const child=layout.nodes.find(n=>n.table.name===relation.child)!;
              const pair=visibleRelations.filter(r=>r.parent===relation.parent&&r.child===relation.child);
              const lane=pair.findIndex(r=>r.id===relation.id);
              const geometry=relationshipPath(parent,child,lane);
              const active=selectedRelation===relation.id||selected===relation.parent||selected===relation.child;
              const inspect=()=>inspectRelation(relation);
              const displayLabel=pair.length>1?relation.label:relation.parent===relation.child?'表示上の親':null;
              const segments=geometry.points.slice(0,-1).map((p,i)=>({a:p,b:geometry.points[i+1],length:Math.hypot(geometry.points[i+1][0]-p[0],geometry.points[i+1][1]-p[1])}));
              const segment=relation.parent===relation.child?segments[2]:segments.sort((a,b)=>b.length-a.length)[0];
              const labelPoint=[(segment.a[0]+segment.b[0])/2,(segment.a[1]+segment.b[1])/2];
              return <g key={relation.id} className={'er-relation '+(active?'is-active':'')+(selected&&!active?' is-muted':'')}>
                <title>{relation.child+'.'+relation.childField+' → '+relation.parent+'.'+relation.parentField+' / '+(relation.optional?'0..1':'1')+' 対 0..N'}</title>
                <path className="er-edge" d={pathString(geometry.points)} markerStart={relation.optional?'url(#er-optional)':'url(#er-one)'} markerEnd="url(#er-many)"/>
                <path className="er-edge-hit" d={pathString(geometry.points)} role="button" tabIndex={0} aria-label={relation.child+'.'+relation.childField+' の関連を確認'} onClick={inspect} onKeyDown={e=>{if(e.key==='Enter'||e.key===' '){e.preventDefault();inspect();}}}/>
                <text className="er-cardinality" x={geometry.start[0]} y={geometry.start[1]} textAnchor="middle">{relation.optional?'0..1':'1'}</text>
                <text className="er-cardinality" x={geometry.end[0]} y={geometry.end[1]} textAnchor="middle">0..N</text>
                {displayLabel&&<text className="er-edge-label" x={labelPoint[0]} y={labelPoint[1]+4} textAnchor="middle">{displayLabel}</text>}
              </g>;
            })}
          </svg>
          {layout.nodes.map(node=>{
            const {table}=node;
            const status=reviews.decisions.find(d=>d.targetId===table.specId)?.status??'unreviewed';
            const comments=reviews.comments.filter(c=>c.targetId===table.specId&&!c.parentId&&c.status==='open').length;
            return <article className={'er-table '+(selected===table.name?'is-selected':'')+(selected&&!focusedNames.has(table.name)?' is-unrelated':'')} key={table.name} style={{left:node.x,top:node.y,width:node.width,height:node.height}}>
              <button className="er-table-heading" aria-label={table.name+' の関係を強調'} aria-pressed={selected===table.name} onClick={()=>selectTable(table.name)}><strong>{table.name}</strong><small>{table.label}</small></button>
              <ul aria-label={table.name+' のカラム'}>{node.fields.map(field=><li className={field.foreign?'is-fk':''} key={field.name} title={field.description+' / '+field.constraint}><span className="er-key">{field.primary&&<b>PK</b>}{field.foreign&&<b>FK</b>}</span><code>{field.name}</code><span className="er-field-type">{field.type}</span>{field.foreign&&/\bNULL\b/.test(field.constraint)&&<span className="er-null">?</span>}</li>)}</ul>
              <button className="er-table-review" onClick={()=>open(table.specId)} aria-label={table.name+' の仕様・コメントを開く'}><span>{status==='approved'?'承認済み':status==='changes'?'変更が必要':'未レビュー'}{comments>0&&' · コメント'+comments}</span><span>仕様・コメント</span></button>
            </article>;
          })}
        </div>
      </div>
    </div>
    {picked&&<div className="er-inspector" aria-live="polite">
      <div className="er-inspector-heading"><h3><code>{picked.name}</code></h3><button onClick={()=>{setSelected(null);setSelectedRelation(null);}}>選択解除</button></div>
      <div className="er-relation-list">{related.map(relation=><button key={relation.id} className={selectedRelation===relation.id?'is-selected':''} onClick={()=>inspectRelation(relation)}>
        <code>{relation.child+'.'+relation.childField+' → '+relation.parent+'.'+relation.parentField}</code>
        <span>{relation.label}</span>
        {(!tableNames.has(relation.parent)||!tableNames.has(relation.child))&&<small>全体図</small>}
      </button>)}</div>
      {!related.length&&<p>参照なし</p>}
      <button className="er-open-spec" onClick={()=>open(picked.specId)}>仕様・コメント</button>
    </div>}
  </section>;
}
