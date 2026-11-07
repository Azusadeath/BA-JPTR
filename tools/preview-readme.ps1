param([Parameter(Mandatory=$true)][string]$Destination)
$ErrorActionPreference='Stop'
$root=Split-Path $PSScriptRoot -Parent
$out=[IO.Path]::GetFullPath($Destination)
New-Item -ItemType Directory -Path $out -Force|Out-Null
$markdown=[IO.File]::ReadAllText((Join-Path $root 'README.md'),[Text.Encoding]::UTF8)
function Inline([string]$value){
    $value=[Net.WebUtility]::HtmlEncode($value)
    $value=[regex]::Replace($value,'`([^`]+)`','<code>$1</code>')
    $value=[regex]::Replace($value,'\*\*(.+?)\*\*','<strong>$1</strong>')
    $value=[regex]::Replace($value,'\[([^\]]+)\]\(([^)]+)\)','<a href="$2">$1</a>')
    return $value
}
$html=[Collections.Generic.List[string]]::new()
$toc=[Collections.Generic.List[string]]::new()
$paragraph=[Collections.Generic.List[string]]::new()
$list=''
$section=0
function Flush-Paragraph {
    if($paragraph.Count -gt 0){$html.Add('<p>'+(Inline ($paragraph -join ' '))+'</p>');$paragraph.Clear()}
}
function Close-List {
    if($script:list){$html.Add('</'+$script:list+'>');$script:list=''}
}
foreach($line in ($markdown -split '\r?\n')){
    if(-not $line.Trim()){Flush-Paragraph;Close-List;continue}
    if($line -match '^(#{1,6}) (.+)$'){
        Flush-Paragraph;Close-List
        $level=$Matches[1].Length;$title=$Matches[2];$id='title'
        if($level -eq 2){$section++;$id='section-'+$section;$toc.Add('<a href="#'+$id+'">'+[Net.WebUtility]::HtmlEncode($title)+'</a>')}
        $html.Add('<h'+$level+' id="'+$id+'">'+(Inline $title)+'</h'+$level+'>');continue
    }
    if($line -match '^> ?(.*)$'){
        Flush-Paragraph;Close-List;$html.Add('<blockquote><p>'+(Inline $Matches[1])+'</p></blockquote>');continue
    }
    if($line -match '^(?:- |\d+\. )(.+)$'){
        Flush-Paragraph;$value=$Matches[1];$kind=if($line.StartsWith('- ')){'ul'}else{'ol'}
        if($list -ne $kind){Close-List;$list=$kind;$html.Add('<'+$kind+'>')}
        $html.Add('<li>'+(Inline $value)+'</li>');continue
    }
    Close-List;$paragraph.Add($line)
}
Flush-Paragraph;Close-List
$template=@'
<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<meta name="color-scheme" content="light">
<title>BA-JPTR · 开源介绍预览</title>
<style>
:root{--bg:#f4f7fb;--ink:#25364d;--muted:#627287;--line:#dce5ef;--blue:#1f69b5;--soft:#edf5fd}
*{box-sizing:border-box}html{scroll-behavior:smooth;scroll-padding-top:90px}body{margin:0;background:var(--bg);color:var(--ink);font:15px/1.75 "Segoe UI","Microsoft YaHei",sans-serif}a{color:#155b9f;text-underline-offset:3px}a:hover{text-decoration:underline}button,a{touch-action:manipulation}button{font:inherit;cursor:pointer}button:focus-visible,a:focus-visible,summary:focus-visible{outline:3px solid #1f69b5;outline-offset:3px}button{border:1px solid var(--line);border-radius:7px;background:#fff;color:var(--ink);padding:7px 13px}.skip{position:fixed;top:-80px;left:20px;background:#fff;padding:10px;z-index:10}.skip:focus{top:10px}
.topbar{position:sticky;top:0;z-index:5;background:rgba(255,255,255,.98);border-bottom:1px solid var(--line)}.bar-inner{max-width:1190px;margin:auto;padding:13px 26px;display:flex;align-items:center;justify-content:space-between;gap:16px}.brand{display:flex;align-items:center;gap:11px;font-size:16px;font-weight:650;color:var(--ink);text-decoration:none}.brand svg{width:32px;height:32px;flex-shrink:0}.draft{margin-left:4px;font-size:12px;font-weight:500;padding:2px 9px;border:1px solid #cbdced;border-radius:4px;color:#456583;background:#f7fbff}.bar-note{font-size:13px;color:var(--muted)}
.shell{max-width:1190px;margin:28px auto 0;padding:0 26px 50px;display:grid;grid-template-columns:204px minmax(0,1fr);gap:30px;align-items:start}.sidebar{position:sticky;top:92px}.side-title{font-weight:650;margin:0 0 12px 12px;font-size:13px}.toc{display:flex;flex-direction:column;border-left:1px solid #d9e3ed}.toc a{padding:7px 12px;text-decoration:none;font-size:13px;color:#56687e;border-left:2px solid transparent;margin-left:-1px;line-height:1.5}.toc a:hover{color:var(--blue);background:#eaf2fb}.toc a[aria-current=true]{color:var(--blue);border-color:var(--blue);font-weight:600}.side-foot{margin:24px 0 0 12px;font-size:12px;color:var(--muted);line-height:1.9}.side-foot strong{display:block;font-weight:500;color:#445a72}
.paper{min-width:0;border:1px solid var(--line);border-radius:10px;background:#fff;overflow:hidden;box-shadow:0 5px 20px #25364d04}.paper-bar{padding:14px 27px;border-bottom:1px solid var(--line);display:flex;align-items:center;justify-content:space-between;gap:12px}.tabs{display:flex;gap:5px}.tabs button{border:0;background:transparent;padding:6px 10px;font-size:13px;color:var(--muted)}.tabs button[aria-selected=true]{background:var(--soft);color:var(--blue);font-weight:650}.copy{font-size:13px;padding:5px 12px}.doc{padding:34px 42px 38px;font-size:15.5px;line-height:1.95;overflow-wrap:anywhere}.doc h1{font-size:29px;line-height:1.45;letter-spacing:.01em;margin:0 0 10px;color:#22364c}.doc h1+p{font-size:13px;color:var(--muted);margin-top:0}.doc h2{font-size:21px;line-height:1.5;margin:37px 0 15px;padding-bottom:9px;border-bottom:1px solid #e8edf3;scroll-margin-top:92px;letter-spacing:.01em}.doc p{margin:13px 0}.doc strong{font-weight:650}.doc li{padding-left:3px;margin:8px 0}.doc ul,.doc ol{padding-left:24px}.doc blockquote{margin:17px 0;padding:12px 18px;border-left:3px solid var(--blue);background:#f0f6fc;border-radius:0 7px 7px 0;color:#254f78}.doc blockquote p{margin:0}.doc code{font-size:.9em;background:#f2f5f8;padding:2px 5px;border-radius:4px}.doc a{overflow-wrap:anywhere}
.raw{margin:0;padding:32px 35px;font:13px/1.9 Consolas,"Microsoft YaHei",monospace;white-space:pre-wrap;overflow-wrap:anywhere;tab-size:4;background:#fbfcfe}.mobile-toc{display:none}.plan{margin-top:22px;border-top:1px solid var(--line);padding:18px 0 0;color:var(--muted);font-size:13px}.plan summary{cursor:pointer;font-weight:600;color:var(--ink);padding:4px 0}.plan dl{display:grid;grid-template-columns:84px minmax(0,1fr);gap:8px 10px;line-height:1.8}.plan dt{color:var(--muted)}.plan dd{margin:0;color:#3c526a;overflow-wrap:anywhere}.plan .files{display:flex;flex-wrap:wrap;gap:10px;margin:14px 0 8px}.plan .files a{padding:7px 12px;border:1px solid #cedced;border-radius:7px;text-decoration:none;background:#fff}.plan .files a:hover{border-color:var(--blue);color:var(--blue)}.foot{font-size:12px;color:var(--muted);margin:19px 0 0;line-height:1.8}.toast{position:fixed;right:25px;bottom:24px;background:#263e57;color:white;padding:11px 17px;border-radius:8px;box-shadow:0 6px 20px #25364d22;max-width:calc(100vw - 40px);z-index:9}.toast:empty{display:none}[hidden]{display:none!important}
@media(max-width:900px){.shell{display:block;max-width:820px;padding:0 17px 35px;margin-top:18px}.sidebar{display:none}.mobile-toc{display:block;margin:0 0 24px;font-size:14px;color:var(--muted)}.mobile-toc summary{cursor:pointer;padding:3px 0}.mobile-toc .toc{margin:12px 0}.bar-inner{padding:12px 18px}.doc{padding:26px 30px}.paper-bar{padding:12px 20px}.bar-note{font-size:12px}.doc h1{font-size:26px}.doc h2{font-size:20px}.plan{margin-top:17px}}
@media(max-width:480px){.bar-note{display:none}.brand{font-size:15px}.shell{padding:0 10px 28px}.paper{border-radius:8px}.paper-bar{padding:11px 12px;gap:6px}.copy{padding:5px 8px;font-size:12px}.tabs{gap:0}.tabs button{padding:6px 9px}.doc{padding:24px 20px;font-size:15px}.doc h1{font-size:24px}.doc h2{margin-top:30px}.raw{padding:20px;font-size:12px}.plan dl{grid-template-columns:72px minmax(0,1fr)}.toast{right:15px;bottom:15px}}
@media(prefers-reduced-motion:reduce){html{scroll-behavior:auto}}
@media print{body{background:white}.topbar,.sidebar,.paper-bar,.mobile-toc,.plan,.foot{display:none}.shell{display:block;margin:0;padding:0}.paper{border:0;box-shadow:none}.doc{padding:0;font-size:11pt}.doc h2{break-after:avoid}}
</style>
</head>
<body>
<a class="skip" href="#readme">跳到正文</a>
<header class="topbar"><div class="bar-inner">
<a class="brand" href="#title"><svg viewBox="0 0 32 32" fill="none" aria-hidden="true"><rect width="32" height="32" rx="8" fill="#1f69b5"/><path d="M8 11.5h16m-4-4 4 4-4 4M24 21H8m4-4-4 4 4 4" stroke="white" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"/></svg>BA-JPTR<span class="draft">发布前预览</span></a>
<span class="bar-note">仅在本地 · 尚未发布到 GitHub</span>
</div></header>
<div class="shell">
<aside class="sidebar" aria-label="文章目录"><p class="side-title">本文目录</p><nav class="toc">{{TOC}}</nav><p class="side-foot">README.md<strong>开源介绍与使用说明</strong>v0.3.2 · Windows x64</p></aside>
<main id="readme">
<div class="paper">
<div class="paper-bar"><div class="tabs" role="tablist" aria-label="查看方式"><button id="read-tab" type="button" role="tab" aria-selected="true" aria-controls="rendered">阅读预览</button><button id="source-tab" type="button" role="tab" aria-selected="false" aria-controls="source">Markdown 原文</button></div><button id="copy" class="copy" type="button">复制 Markdown</button></div>
<article id="rendered" class="doc" role="tabpanel" aria-labelledby="read-tab"><details class="mobile-toc"><summary>展开目录</summary><nav class="toc">{{TOC}}</nav></details>{{BODY}}</article>
<pre id="source" class="raw" role="tabpanel" aria-labelledby="source-tab" tabindex="0" hidden>{{SOURCE}}</pre>
</div>
<details class="plan"><summary>待发布内容</summary><dl><dt>拟用仓库</dt><dd>Azusadeath/BA-JPTR（名称可调整）</dd><dt>仓库介绍</dt><dd>适用于已安装汉化补丁的蔚蓝档案日服 PC 客户端的译名替换工具，支持来源筛选、单条及批量修改、预览与恢复。</dd><dt>开源许可</dt><dd>GPL-3.0-only，保留上游署名与第三方许可。</dd><dt>上传范围</dt><dd>应用源码、文档、构建脚本和许可文件。个人词表、游戏底稿、游戏资源、开发工具链与构建缓存不上传。</dd><dt>发行版本</dt><dd>v0.3.2 · Windows x64；程序与配套源码分别打包。</dd></dl><div class="files"><a href="release/蔚蓝档案日服译名替换-v0.3.2-Windows-x64.zip" download>下载待发布的用户 ZIP</a><a href="release/bajptr-0.3.2-source.zip" download>下载配套源码 ZIP</a><a href="RELEASE_NOTES.md">查看发行说明草稿</a></div></details>
<p class="foot">此页用于审阅介绍与发布内容。确认后，在聊天中告诉我再发布；本页不会创建仓库或上传文件。</p>
</main></div><div id="toast" class="toast" role="status" aria-live="polite"></div>
<script>
const reading=document.getElementById('rendered'),source=document.getElementById('source'),readTab=document.getElementById('read-tab'),sourceTab=document.getElementById('source-tab');
function view(raw){reading.hidden=raw;source.hidden=!raw;readTab.setAttribute('aria-selected',String(!raw));sourceTab.setAttribute('aria-selected',String(raw));}
readTab.addEventListener('click',()=>view(false));sourceTab.addEventListener('click',()=>view(true));
document.querySelectorAll('.toc a,.brand,.skip').forEach(link=>link.addEventListener('click',()=>view(false)));
let toastTimer;
document.getElementById('copy').addEventListener('click',async()=>{let message='Markdown 已复制';try{await navigator.clipboard.writeText(source.textContent);}catch{view(true);source.focus();message='请在原文视图中选择并复制。';}const toast=document.getElementById('toast');toast.textContent=message;clearTimeout(toastTimer);toastTimer=setTimeout(()=>toast.textContent='',3000);});
let scrollFrame=0;function highlightSection(){scrollFrame=0;const headings=[...reading.querySelectorAll('h2')];let id=headings[0]?.id;for(const heading of headings){if(heading.getBoundingClientRect().top>140)break;id=heading.id;}document.querySelectorAll('.toc a').forEach(link=>link.setAttribute('aria-current',String(link.hash==='#'+id)));}function queueHighlight(){if(!scrollFrame)scrollFrame=requestAnimationFrame(highlightSection);}window.addEventListener('scroll',queueHighlight,{passive:true});window.addEventListener('resize',queueHighlight);highlightSection();
</script>
</body></html>
'@
$page=$template.Replace('{{TOC}}',($toc -join "`n")).Replace('{{BODY}}',($html -join "`n")).Replace('{{SOURCE}}',[Net.WebUtility]::HtmlEncode($markdown))
[IO.File]::WriteAllText((Join-Path $out 'index.html'),$page,[Text.UTF8Encoding]::new($false))
Copy-Item -LiteralPath (Join-Path $root 'README.md') -Destination (Join-Path $out 'README.md') -Force
Write-Host ('Local preview: '+(Join-Path $out 'index.html'))
