import { useCallback, useEffect, useRef, useState, type FormEvent } from 'react';
import type { Catalog, ReviewState, Spec } from '../types';
import DatabaseDiagram from './DatabaseDiagram';
import FlowDiagram from './FlowDiagram';
import { architectureFlow } from './flow-model';

type Page = 'requirements' | 'infrastructure' | 'database' | 'functions' | 'decisions' | 'reviews' | 'sources';
type Status = ReviewState['decisions'][number]['status'];
const statusLabels = { unreviewed: '未レビュー', approved: '承認済み', changes: '変更が必要' };
const kindLabels = { question: '質問', change: '変更依頼', note: 'メモ' };
const navigation = [
  {id:'requirements',label:'機能と要件'},
  {id:'infrastructure',label:'インフラ構成'},
  {id:'database',label:'データベース'},
  {id:'functions',label:'関数と処理フロー'},
  {id:'decisions',label:'未決事項'},
  {id:'reviews',label:'レビュー'},
  {id:'sources',label:'Figma と根拠'},
] as const;
const emptyReviews: ReviewState = {comments:[],decisions:[]};

function currentRoute() {
  const [pagePart,query] = window.location.hash.slice(2).split('?');
  return {
    page: navigation.some(item=>item.id===pagePart) ? pagePart as Page : 'requirements' as Page,
    selected:new URLSearchParams(query).get('spec'),
  };
}
async function request<T>(path:string,method='GET',body?:unknown):Promise<T> {
  const response=await fetch(path,{
    method, headers:body?{'Content-Type':'application/json'}:undefined,
    body:body?JSON.stringify(body):undefined,
  });
  const data=await response.json();
  if(!response.ok)throw new Error(data.error??'接続に失敗しました');
  return data as T;
}
function getStatus(spec:Spec,reviews:ReviewState):Status {
  return reviews.decisions.find(item=>item.targetId===spec.id)?.status??'unreviewed';
}
const time=(value:string)=>new Intl.DateTimeFormat('ja-JP',{
  month:'numeric',day:'numeric',hour:'2-digit',minute:'2-digit',
}).format(new Date(value));

export default function Workspace() {
  const [catalog,setCatalog]=useState<Catalog|null>(null);
  const [reviews,setReviews]=useState<ReviewState>(emptyReviews);
  const [route,setRoute]=useState(currentRoute);
  const [query,setQuery]=useState('');
  const [error,setError]=useState('');
  const [connected,setConnected]=useState(false);
  const [saving,setSaving]=useState(false);
  const [mobileNav,setMobileNav]=useState(false);
  const searchRef=useRef<HTMLInputElement>(null);
  const refresh=useCallback(async()=>{
    try {setReviews(await request<ReviewState>('/api/reviews'));setConnected(true);}
    catch {setConnected(false);}
  },[]);
  useEffect(()=>{window.scrollTo(0,0);},[route.page]);
  useEffect(()=>{
    Promise.all([request<Catalog>('/api/catalog'),request<ReviewState>('/api/reviews')])
      .then(([data,state])=>{setCatalog(data);setReviews(state);setConnected(true);})
      .catch(e=>setError(e.message));
    const hashChange=()=>{setRoute(currentRoute());setMobileNav(false);};
    const shortcut=(e:KeyboardEvent)=>{
      if((e.metaKey||e.ctrlKey)&&e.key==='k'){e.preventDefault();searchRef.current?.focus();}
    };
    window.addEventListener('hashchange',hashChange);
    window.addEventListener('focus',refresh);
    window.addEventListener('keydown',shortcut);
    const timer=setInterval(refresh,15000);
    return ()=>{
      window.removeEventListener('hashchange',hashChange);
      window.removeEventListener('focus',refresh);
      window.removeEventListener('keydown',shortcut);
      clearInterval(timer);
    };
  },[refresh]);
  const navigate=(page:Page)=>{
    setQuery('');setMobileNav(false);
    window.location.hash='/'+page;window.scrollTo(0,0);
  };
  const open=(id:string)=>{
    const spec=[...(catalog?.specs??[]),...(catalog?.archivedSpecs??[])].find(item=>item.id===id);
    if(spec)window.location.hash='/'+spec.category+'?spec='+encodeURIComponent(id);
  };
  const close=()=>{window.location.hash='/'+route.page;};
  const mutate=async(path:string,method:string,body:unknown)=>{
    setSaving(true);
    try {setReviews(await request<ReviewState>(path,method,body));setConnected(true);}
    finally {setSaving(false);}
  };
  if(!catalog)return <main className="loading-screen">
    {error?<><p role="alert">{error}</p><button onClick={()=>location.reload()}>再読み込み</button></>:<p>読み込み中…</p>}
  </main>;

  const selected=[...catalog.specs,...(catalog.archivedSpecs??[])].find(item=>item.id===route.selected);
  const searchTerm=query.toLocaleLowerCase().trim();
  const searchResults=searchTerm?catalog.specs.filter(spec=>[
    spec.title,spec.subtitle,spec.summary,spec.signature,
    ...spec.details.map(d=>d.title+' '+d.body),
    ...(spec.fields??[]).map(f=>Object.values(f).join(' ')),
    ...spec.sources.map(id=>{const source=catalog.sources.find(s=>s.id===id);return source?source.title+' '+source.body:'';}),
  ].join(' ').toLocaleLowerCase().includes(searchTerm)):[];
  const unresolved=reviews.comments.filter(item=>item.status==='open'&&!item.parentId).length;
  const pageTitle=navigation.find(item=>item.id===route.page)!.label;

  return <div className="app-shell">
    <aside className={'sidebar '+(mobileNav?'mobile-open':'')}>
      <a href="#/requirements" className="brand" onClick={e=>{e.preventDefault();navigate('requirements');}}>omnooth</a>
      <nav aria-label="設計">
        {navigation.map(({id,label})=><button key={id} className={'nav-item '+(id===route.page?'active':'')}
          aria-current={id===route.page?'page':undefined} onClick={()=>navigate(id)}>
          {label}{id==='reviews'&&unresolved>0&&<span>{unresolved}</span>}
        </button>)}
      </nav>
      <div className="sidebar-links">
        <a href="https://www.figma.com/design/aKMF6XjaOU2Hz7Bj9GxPXY/omnooth?m=dev" target="_blank" rel="noreferrer">Figma</a>
        <a href="/api/export" download>レビューを書き出す</a>
      </div>
    </aside>
    {mobileNav&&<button className="nav-scrim" aria-label="メニューを閉じる" onClick={()=>setMobileNav(false)}/>}
    <div className="workspace">
      <header className="app-header">
        <button className="mobile-menu" onClick={()=>setMobileNav(!mobileNav)}>メニュー</button>
        <input ref={searchRef} className="global-search" value={query} onChange={e=>setQuery(e.target.value)}
          placeholder="検索" aria-label="設計を検索"/>
        {query&&<button onClick={()=>setQuery('')}>クリア</button>}
        {saving&&<span role="status">保存中…</span>}
      </header>
      {!connected&&<div className="connection-banner" role="alert">接続できません。<button onClick={refresh}>再接続</button></div>}
      <main className="page-content">
        <h1>{searchTerm?'検索結果':pageTitle}</h1>
        {searchTerm?<SpecList specs={searchResults} {...{reviews,open}} empty="該当なし"/>
        :route.page==='functions'?<FunctionsPage {...{catalog,reviews,open}}/>
        :route.page==='database'?<>
          <DatabaseDiagram {...{catalog,reviews,open}}/>
          <details className="database-spec-list"><summary>テーブル仕様一覧</summary>
            <SpecList specs={catalog.specs.filter(s=>s.category==='database')} {...{reviews,open}}/>
          </details>
        </>
        :route.page==='reviews'?<ReviewsPage {...{catalog,reviews,open}}/>
        :route.page==='sources'?<SourcesPage {...{catalog,open}}/>
        :<>
          {route.page==='infrastructure'&&<ArchitectureMap {...{catalog,open}}/>}
          <SpecList specs={catalog.specs.filter(s=>s.category===route.page)} {...{reviews,open}}/>
        </>}
      </main>
    </div>
    {selected&&<SpecDetail key={selected.id} spec={selected} {...{catalog,reviews,open,close,mutate,saving,connected}}/>}
  </div>;
}

function ArchitectureMap({catalog,open}:{catalog:Catalog;open:(id:string)=>void}) {
  return <FlowDiagram flow={architectureFlow} {...{catalog,open}}/>;
}
function SpecList({specs,reviews,open,empty='該当なし'}:{
  specs:Spec[];reviews:ReviewState;open:(id:string)=>void;empty?:string;
}) {
  const [filter,setFilter]=useState('all');
  const filtered=specs.filter(spec=>filter==='all'||getStatus(spec,reviews)===filter);
  return <section className="spec-list-section">
    <div className="list-toolbar"><select aria-label="設計のレビュー状態で絞り込む" value={filter} onChange={e=>setFilter(e.target.value)}>
      <option value="all">すべて</option><option value="unreviewed">未レビュー</option>
      <option value="approved">承認済み</option><option value="changes">変更が必要</option>
    </select></div>
    <div className="spec-list">{filtered.map(spec=>{
      const count=reviews.comments.filter(c=>c.targetId===spec.id&&c.status==='open'&&!c.parentId).length;
      return <button className="spec-row" key={spec.id} onClick={()=>open(spec.id)}>
        <span className="spec-row-content">
          <strong className={spec.category==='functions'||spec.category==='database'?'mono':''}>{spec.title}</strong>
          <span className="spec-summary">{spec.summary}</span>
        </span>
        <span className="row-review">{statusLabels[getStatus(spec,reviews)]}{count>0&&<span>コメント {count}</span>}</span>
      </button>;
    })}</div>
    {!filtered.length&&<p className="empty-state">{empty}</p>}
  </section>;
}
function FunctionsPage({catalog,reviews,open}:{catalog:Catalog;reviews:ReviewState;open:(id:string)=>void}) {
  const [flowId,setFlowId]=useState('install');
  const flow=catalog.flows.find(f=>f.id===flowId)!;
  return <>
    <div className="filter-tabs" role="tablist" aria-label="処理シナリオ">
      {catalog.flows.map(f=><button role="tab" aria-selected={f.id===flowId} key={f.id} onClick={()=>setFlowId(f.id)}>{f.trigger}</button>)}
    </div>
    <FlowDiagram {...{flow,catalog,open}}/>
    <details className="function-spec-list"><summary>関数一覧</summary>
      <SpecList specs={catalog.specs.filter(s=>s.category==='functions')} {...{reviews,open}}/>
    </details>
  </>;
}
function ReviewsPage({catalog,reviews,open}:{catalog:Catalog;reviews:ReviewState;open:(id:string)=>void}) {
  const [filter,setFilter]=useState('open');
  const list=reviews.comments.filter(c=>!c.parentId&&(filter==='all'||c.status===filter));
  return <>
    <div className="filter-tabs">{[['open','未解決'],['resolved','解決済み'],['all','すべて']].map(([id,label])=>
      <button key={id} aria-pressed={filter===id} onClick={()=>setFilter(id)}>{label}</button>)}</div>
    <div className="inbox-list">{list.map(comment=><button className="inbox-row" key={comment.id} onClick={()=>open(comment.targetId)}>
      <strong>{[...catalog.specs,...(catalog.archivedSpecs??[])].find(s=>s.id===comment.targetId)?.title??comment.targetId}</strong>
      <span className="comment-meta">{comment.author} · {kindLabels[comment.kind]} · {time(comment.createdAt)}</span>
      <span className="comment-text">{comment.body}</span>
    </button>)}</div>
    {!list.length&&<p className="empty-state">該当なし</p>}
  </>;
}
function SourcesPage({catalog,open}:{catalog:Catalog;open:(id:string)=>void}) {
  const [filter,setFilter]=useState('screen');
  return <>
    <div className="filter-tabs">{[['screen','画面'],['comment','コメント'],['request','追加要件'],['docs','技術資料']].map(([id,label])=>
      <button key={id} aria-pressed={filter===id} onClick={()=>setFilter(id)}>{label}</button>)}</div>
    <div className="source-list">{catalog.sources.filter(s=>s.kind===filter).map(source=><article key={source.id}>
      <h2>{source.url?<a href={source.url} target="_blank" rel="noreferrer">{source.title}</a>:source.title}</h2>
      <p>{source.body}</p>
      <div className="source-linked-specs">{catalog.specs.filter(s=>s.sources.includes(source.id)).map(spec=>
        <button key={spec.id} onClick={()=>open(spec.id)}>{spec.title}</button>)}</div>
    </article>)}</div>
  </>;
}

function SpecDetail({spec,catalog,reviews,open,close,mutate,saving,connected}:{
  spec:Spec;catalog:Catalog;reviews:ReviewState;open:(id:string)=>void;close:()=>void;
  mutate:(path:string,method:string,body:unknown)=>Promise<void>;saving:boolean;connected:boolean;
}) {
  const [error,setError]=useState('');
  const [replyTo,setReplyTo]=useState<string|null>(null);
  const [showResolved,setShowResolved]=useState(!!spec.archived);
  const dialogRef=useRef<HTMLDivElement>(null);
  const onClose=useRef(close);
  onClose.current=close;
  useEffect(()=>{
    const previous=document.activeElement as HTMLElement|null;
    const overflow=document.body.style.overflow;
    document.body.style.overflow='hidden';
    dialogRef.current?.focus();
    const key=(e:KeyboardEvent)=>{
      if(e.key==='Escape'){e.preventDefault();onClose.current();}
      if(e.key==='Tab'){
        const nodes=dialogRef.current?.querySelectorAll<HTMLElement>('button:not([disabled]),a[href],input,textarea,select,[tabindex="0"]');
        if(!nodes?.length)return;
        const first=nodes[0],last=nodes[nodes.length-1];
        if(e.shiftKey&&(document.activeElement===first||document.activeElement===dialogRef.current)){e.preventDefault();last.focus();}
        else if(!e.shiftKey&&document.activeElement===last){e.preventDefault();first.focus();}
      }
    };
    window.addEventListener('keydown',key);
    return ()=>{window.removeEventListener('keydown',key);document.body.style.overflow=overflow;previous?.focus();};
  },[]);
  const perform=async(path:string,method:string,body:unknown)=>{
    setError('');
    try {await mutate(path,method,body);return true;}
    catch(e){setError((e as Error).message);return false;}
  };
  const comments=reviews.comments.filter(c=>c.targetId===spec.id);
  const roots=comments.filter(c=>!c.parentId);
  const status=getStatus(spec,reviews);
  const hasOpenChange=comments.some(c=>c.status==='open'&&c.kind==='change');

  return <div className="detail-overlay">
    <button className="detail-backdrop" aria-label="詳細を閉じる" tabIndex={-1} onClick={close}/>
    <div className="detail-drawer" role="dialog" aria-modal="true" aria-labelledby="detail-title" tabIndex={-1} ref={dialogRef}>
      <header className="detail-toolbar"><button onClick={close}>閉じる</button></header>
      <div className="detail-scroll">
        <h1 id="detail-title" className={spec.category==='functions'||spec.category==='database'?'mono':''}>{spec.title}</h1>
        <p className="spec-origin">{spec.origin}</p>
        {(spec.category==='functions'||spec.category==='database')&&<p className="detail-subtitle">{spec.subtitle}</p>}
        <p className="detail-summary">{spec.summary}</p>
        {spec.signature&&<pre className="signature"><code>{spec.signature}</code></pre>}
        {spec.fields&&<section className="detail-section">
          <h2>カラム</h2>
          <div className="table-scroll"><table><thead><tr><th>カラム</th><th>型</th><th>制約</th><th>内容</th></tr></thead>
            <tbody>{spec.fields.map(field=><tr key={field.name}><td><code>{field.name}</code></td><td>{field.type}</td><td>{field.constraint}</td><td>{field.description}</td></tr>)}</tbody>
          </table></div>
        </section>}
        {spec.details.map(detail=><section className="detail-section" key={detail.title}><h2>{detail.title}</h2><p>{detail.body}</p></section>)}
        {!!spec.calls?.length&&<section className="detail-section"><h2>呼び出し先</h2>
          <div className="linked-specs">{spec.calls.map(id=><button key={id} onClick={()=>open(id)}><code>{catalog.specs.find(s=>s.id===id)?.title}</code></button>)}</div>
        </section>}
        {spec.acceptance.length>0&&<section className="detail-section"><h2>完了条件</h2><ul className="acceptance">{spec.acceptance.map(check=><li key={check}>{check}</li>)}</ul></section>}
        {spec.related.length>0&&<section className="detail-section"><h2>関連</h2><div className="linked-specs">
          {spec.related.map(id=>{const item=catalog.specs.find(s=>s.id===id);return item&&<button key={id} onClick={()=>open(id)}>{item.title}</button>;})}
        </div></section>}
        {spec.sources.length>0&&<section className="detail-section"><h2>根拠</h2><ul className="source-references">
          {spec.sources.map(id=>{const source=catalog.sources.find(s=>s.id===id);return source&&<li key={id}>{source.url?<a href={source.url} target="_blank" rel="noreferrer">{source.title}</a>:source.title}</li>;})}
        </ul></section>}
        <section className="detail-section comments-section">
          <div className="comments-heading"><h2>コメント{roots.length>0&&' ('+roots.length+')'}</h2>
            {roots.some(c=>c.status==='resolved')&&<label><input type="checkbox" checked={showResolved} onChange={e=>setShowResolved(e.target.checked)}/>解決済みを表示</label>}
          </div>
          {roots.filter(c=>showResolved||c.status==='open').map(comment=><article className={'comment-thread '+comment.status} key={comment.id}>
            <CommentBody comment={comment}/>
            {comments.filter(c=>c.parentId===comment.id).map(reply=><div className="comment-reply" key={reply.id}><CommentBody comment={reply}/></div>)}
            <div className="comment-actions">
              <button onClick={()=>setReplyTo(replyTo===comment.id?null:comment.id)} disabled={saving||!connected}>返信</button>
              <button onClick={()=>perform('/api/comments/'+comment.id,'PATCH',{status:comment.status==='open'?'resolved':'open'})} disabled={saving||!connected}>
                {comment.status==='open'?'解決にする':'再オープン'}
              </button><span>{comment.status==='resolved'?'解決済み':'未解決'}</span>
            </div>
            {replyTo===comment.id&&<CommentForm key={comment.id} {...{spec,saving,connected}} parentId={comment.id} onSubmit={async(input)=>{
              const ok=await perform('/api/comments','POST',input);if(ok)setReplyTo(null);return ok;
            }}/>}
          </article>)}
          <CommentForm key={'new-'+spec.id} {...{spec,saving,connected}} onSubmit={input=>perform('/api/comments','POST',input)}/>
        </section>
        {hasOpenChange&&<p className="review-blocker">未解決の変更依頼があります。</p>}
        {error&&<p className="form-error" role="alert">{error}</p>}
      </div>
      {!spec.archived&&<footer className="approval-controls">
        {(['unreviewed','changes','approved'] as const).map(value=><button key={value} aria-pressed={status===value}
          disabled={saving||!connected||(value==='approved'&&hasOpenChange)}
          onClick={()=>perform('/api/decisions/'+spec.id,'PATCH',{status:value,revision:spec.revision})}>
          {statusLabels[value]}
        </button>)}
      </footer>}
    </div>
  </div>;
}
function CommentBody({comment}:{comment:ReviewState['comments'][number]}) {
  return <><div className="comment-meta"><span>{comment.author}</span><span>{kindLabels[comment.kind]}</span>
    <time dateTime={comment.createdAt}>{time(comment.createdAt)}</time></div><p className="comment-text">{comment.body}</p></>;
}
function CommentForm({spec,parentId,saving,connected,onSubmit}:{
  spec:Spec;parentId?:string;saving:boolean;connected:boolean;onSubmit:(input:unknown)=>Promise<boolean>;
}) {
  const draftKey='omnooth-draft:'+spec.id+':'+(parentId??'new');
  const [body,setBody]=useState(()=>localStorage.getItem(draftKey)??'');
  const [kind,setKind]=useState<'question'|'change'|'note'>('question');
  const submit=async(e:FormEvent)=>{
    e.preventDefault();
    const ok=await onSubmit({
      targetId:spec.id,revision:spec.revision,
      author:localStorage.getItem('omnooth-review-author')?.trim()||'tukimi',
      body:body.trim(),kind,parentId:parentId??null,
    });
    if(ok){setBody('');localStorage.removeItem(draftKey);}
  };
  return <form className="comment-form" onSubmit={submit}>
    <select value={kind} onChange={e=>setKind(e.target.value as typeof kind)} aria-label={parentId?'返信の種別':'コメントの種別'}>
      <option value="question">質問</option>{!spec.archived&&<option value="change">変更依頼</option>}<option value="note">メモ</option>
    </select>
    <textarea value={body} maxLength={10000} required aria-label={parentId?'返信内容':'コメント内容'}
      placeholder={parentId?'返信':'コメント'}
      onChange={e=>{setBody(e.target.value);localStorage.setItem(draftKey,e.target.value);}}/>
    <button type="submit" aria-label={parentId?'返信を保存':'コメントを保存'} disabled={saving||!connected||!body.trim()}>
      {parentId?'返信を保存':'保存'}
    </button>
  </form>;
}
