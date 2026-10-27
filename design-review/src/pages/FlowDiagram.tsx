import { useEffect, useId, useMemo, useRef, useState } from 'react';
import type { Catalog } from '../types';
import { createFlowModel } from './flow-model';
import './flow-diagram.css';

export default function FlowDiagram({flow,catalog,open}:{flow:Catalog['flows'][number];catalog:Catalog;open:(id:string)=>void}) {
  const model=useMemo(()=>createFlowModel(flow,catalog.specs),[flow,catalog]);
  const viewport=useRef<HTMLDivElement>(null);
  const [viewportWidth,setViewportWidth]=useState(1000);
  const markerId='flow-arrow-'+useId().replaceAll(':','');
  const scale=Math.max(.85,Math.min(1,viewportWidth/model.width));
  useEffect(()=>{
    if(!viewport.current)return;
    const observer=new ResizeObserver(entries=>setViewportWidth(entries[0].contentRect.width));
    observer.observe(viewport.current);
    return ()=>observer.disconnect();
  },[]);
  useEffect(()=>{viewport.current?.scrollTo(0,0);},[flow.id]);
  return <div className="flow-diagram" ref={viewport} role="region" aria-label={flow.title+'の処理フロー'} tabIndex={0}>
    <div style={{width:model.width*scale,height:model.height*scale}}>
      <div className="flow-canvas" style={{width:model.width,height:model.height,transform:'scale('+scale+')'}}>
        <svg className="flow-connections" width={model.width} height={model.height} aria-label="処理の方向と条件">
          <defs><marker id={markerId} viewBox="0 0 10 10" refX="9" refY="5" markerWidth="9" markerHeight="9" orient="auto-start-reverse"><path d="M 1 1 L 9 5 L 1 9" fill="none" stroke="#171717" strokeWidth="1.5"/></marker></defs>
          {model.edges.map(edge=><g key={edge.id}>
            <title>{model.nodes.find(n=>n.id===edge.from)!.label+' → '+model.nodes.find(n=>n.id===edge.to)!.label+(edge.label?'：'+edge.label:'')}</title>
            <path className={'flow-connection '+(edge.feedback?'flow-feedback':'')} d={edge.path} markerEnd={'url(#'+markerId+')'}/>
            {edge.label&&<text className="flow-edge-label" x={edge.labelAt[0]} y={edge.labelAt[1]} textAnchor="middle">{edge.label}</text>}
          </g>)}
        </svg>
        {model.nodes.map(node=>{
          const content=<>
            {node.kind==='condition'&&<svg className="flow-condition-outline" viewBox="0 0 244 74" aria-hidden="true"><path d="M122 1 L243 37 L122 73 L1 37 Z"/></svg>}
            <strong>{node.label}</strong>{node.caption&&<span>{node.caption}</span>}
          </>;
          const style={left:node.x,top:node.y,width:node.width,height:node.height};
          return node.specId
            ?<button key={node.id} className="flow-node flow-function" style={style} onClick={()=>open(node.specId!)} aria-label={node.label+' の仕様・コメントを開く'}>{content}</button>
            :<div key={node.id} className={'flow-node flow-'+node.kind} style={style}>{content}</div>;
        })}
      </div>
    </div>
  </div>;
}
