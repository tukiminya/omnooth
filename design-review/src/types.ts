export type Spec = {
  archived?: boolean;
  id: string;
  revision: string;
  category: 'requirements' | 'infrastructure' | 'database' | 'functions' | 'decisions';
  title: string;
  subtitle: string;
  origin: 'Figma確認' | 'ユーザー要件' | '技術提案' | '要確認';
  summary: string;
  details: { title: string; body: string }[];
  sources: string[];
  related: string[];
  signature?: string;
  fields?: { name: string; type: string; constraint: string; description: string }[];
  calls?: string[];
  acceptance: string[];
};
export type ReviewState = {
  comments: {
    id: string; targetId: string; revision: string; author: string; body: string;
    kind: 'question' | 'change' | 'note'; status: 'open' | 'resolved';
    parentId: string | null; createdAt: string; updatedAt: string;
  }[];
  decisions: { targetId: string; revision: string; status: 'unreviewed' | 'approved' | 'changes'; updatedAt: string }[];
};
export type Source = {
  id: string; kind: 'screen' | 'comment' | 'docs' | 'request';
  title: string; body: string; url?: string; nodeId?: string;
};
export type Catalog = {
  version: string; title: string; sourceNote: string; specs: Spec[]; sources: Source[];
  archivedSpecs?: Spec[];
  flows: {id: string; title: string; trigger: string; description: string; steps: string[]; result: string}[];
};
