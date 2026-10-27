export type BoothItemMetadata = {
  boothItemId:string;title:string;description:string|null;url:string;ageRating:'all'|'r18';fetchedAt:string;
  shop:{externalKey:string;name:string;url:string};
  images:{originalUrl:string;thumbnailUrl:string|null}[];
  tags:string[];category:{externalKey:string;name:string}|null;
  variations:{variationId:string;name:string|null;downloadables:{filename:string;extension:string;sizeLabel:string|null;updatedAt:string|null}[]}[];
};
function record(value:unknown):Record<string,unknown> {
  if(!value||typeof value!=='object'||Array.isArray(value))throw new Error('INVALID_RESPONSE');
  return value as Record<string,unknown>;
}
function requiredText(value:unknown):string {
  if(typeof value!=='string'||!value.trim())throw new Error('INVALID_RESPONSE');
  return value;
}
function id(value:unknown):string {
  if(typeof value!=='number'||!Number.isSafeInteger(value)||value<=0)throw new Error('INVALID_RESPONSE');
  return String(value);
}
function https(value:unknown,host:(name:string)=>boolean):string {
  const raw=requiredText(value);
  let url:URL;
  try {url=new URL(raw);}catch {throw new Error('INVALID_RESPONSE');}
  if(url.protocol!=='https:'||url.username||url.password||url.port||!host(url.hostname))throw new Error('INVALID_RESPONSE');
  return raw;
}
const boothHost=(name:string)=>name==='booth.pm'||name.endsWith('.booth.pm');
function timestamp(value:unknown):string|null {
  if(typeof value!=='string')return null;
  const date=new Date(value);
  return Number.isFinite(date.getTime())?date.toISOString():null;
}
export function adaptBoothItemMetadata(payload:unknown,input:{expectedItemId:string;fetchedAt:string}):BoothItemMetadata {
  const data=record(payload),shop=record(data.shop);
  const boothItemId=id(data.id);
  if(boothItemId!==input.expectedItemId||typeof data.is_adult!=='boolean')throw new Error('INVALID_RESPONSE');
  return {
    boothItemId,title:requiredText(data.name),description:typeof data.description==='string'?data.description:null,
    url:https(data.url,boothHost),ageRating:data.is_adult?'r18':'all',fetchedAt:requiredText(timestamp(input.fetchedAt)),
    shop:{externalKey:id(shop.id),name:requiredText(shop.name),url:https(shop.url,boothHost)},
    images:(Array.isArray(data.images)?data.images:[]).map(value=>{
      const image=record(value);
      return {originalUrl:https(image.original,name=>name==='booth.pximg.net'),thumbnailUrl:typeof image.resized==='string'?https(image.resized,name=>name==='booth.pximg.net'):null};
    }),
    tags:(Array.isArray(data.tags)?data.tags:[]).map(value=>requiredText(record(value).name)),
    category:data.category?{externalKey:id(record(data.category).id),name:requiredText(record(data.category).name)}:null,
    variations:(Array.isArray(data.variations)?data.variations:[]).map(value=>{
      const variation=record(value);
      return {variationId:id(variation.id),name:typeof variation.name==='string'?variation.name:null,
        downloadables:(Array.isArray(variation.downloadables)?variation.downloadables:[]).map(value=>{
          const file=record(value);
          return {filename:requiredText(file.name),extension:requiredText(file.file_extension),sizeLabel:typeof file.file_size==='string'?file.file_size:null,updatedAt:timestamp(file.updated_at)};
        })};
    }),
  };
}
export function findDownloadUpdatedAt(metadata:BoothItemMetadata,input:{variationId:string;filename:string}):string|null {
  const files=metadata.variations.filter(v=>v.variationId===input.variationId).flatMap(v=>v.downloadables).filter(f=>f.filename===input.filename);
  return files.length===1?files[0].updatedAt:null;
}
