package ovh.kweb.mail.ui.thread

/**
 * Makes a wide part of a mail fit, the way Gmail does it: only the part that sticks out (a table of eight columns that
 * do not wrap, a signature laid out for 600 pixels) is made smaller with CSS `zoom`, until it fits the width it has; the
 * running text around it keeps its size and wraps at the window's width. Scaling the whole page instead (overview mode,
 * then [FitRules]' second and third layouts) made the text small or narrow whenever one part was much wider than the
 * window (owner, 2026-11-25).
 *
 * The script is the app's own, run with `evaluateJavascript` once the page has loaded, and only on the app's own document
 * (see `PanWebView.pageFinished`). The mail itself still runs nothing: the page's content security policy
 * ([HtmlWrapper.POLICY], `script-src 'none'`) blocks every script, event handler and `javascript:` link of the mail, and
 * the view has no JavaScript interface. The script only reads the layout and sets `zoom` on elements; it reads no text
 * and sends nothing. A side effect of scripting being on: a mail's `<noscript>` content is not shown.
 *
 * Walking down from the body: a child whose box ends past the right edge of the space it has is zoomed down until it ends
 * at that edge; a child that fits but has something inside that sticks out is walked into, with its own content box as the
 * space. Plain inline elements and boxes of no size (`display: contents`, a folded quote) are walked into with the space
 * of their parent. A folded quote that is opened later is fitted then (a `toggle` listener of the script's own).
 *
 * The mail's markup can name elements so that `document.body` or a form's `children` point elsewhere (DOM clobbering):
 * the script therefore calls the DOM's own methods and getters from the prototypes, never through the elements.
 */
object FitScript {
    /** How small a part may be made; below this it is left as it is (the page then scales, as before). */
    const val MIN_ZOOM = 0.15

    /** At most this many elements are zoomed and this many visited, so a hostile mail cannot keep the view busy. */
    const val MAX_ZOOMED = 300
    const val MAX_VISITED = 20000

    val JS: String = """
(function(){
  var MIN=$MIN_ZOOM, MAXZ=$MAX_ZOOMED, MAXN=$MAX_VISITED, n=0, z=0;
  var EP=Element.prototype, D=Document.prototype;
  var rect=EP.getBoundingClientRect, kidsOf=Object.getOwnPropertyDescriptor(EP,'children').get;
  var cw=Object.getOwnPropertyDescriptor(EP,'clientWidth').get, sw=Object.getOwnPropertyDescriptor(EP,'scrollWidth').get;
  var cl=Object.getOwnPropertyDescriptor(EP,'clientLeft').get, qs=D.querySelector, gcs=window.getComputedStyle;
  var styleOf=Object.getOwnPropertyDescriptor(HTMLElement.prototype,'style').get, setp=CSSStyleDeclaration.prototype.setProperty;
  function pads(e){var s=gcs(e);return [parseFloat(s.paddingLeft)||0, parseFloat(s.paddingRight)||0];}
  function inner(k){var r=rect.call(k),p=pads(k);return [r.left+cl.call(k)+p[0], cw.call(k)-p[0]-p[1]];}
  function fit(box,left,avail,depth){
    if(depth>60||avail<=0)return;
    var kids=kidsOf.call(box), limit=left+avail;
    for(var i=0;i<kids.length;i++){
      var k=kids[i];
      if(++n>MAXN||z>=MAXZ)return;
      if(!(k instanceof HTMLElement))continue;
      var r=rect.call(k), cs=gcs(k), d=cs.display;
      if(d==='none'||cs.position==='absolute'||cs.position==='fixed')continue;
      if((r.width===0&&r.height===0)||d==='contents'||d==='inline'){fit(k,left,avail,depth+1);continue;}
      if(r.right>limit+1){
        var start=Math.max(r.left,left), f=(limit-start)/r.width;
        if(f>=MIN&&f<1){setp.call(styleOf.call(k),'zoom',String(Math.floor(f*1000)/1000),'important');z++;continue;}
      }
      if(sw.call(k)>cw.call(k)+1){var c=inner(k);fit(k,c[0],c[1],depth+1);}
    }
  }
  var b=qs.call(document,'body');if(!b)return 0;
  function all(){n=0;var c=inner(b);fit(b,c[0],c[1],0);}
  all();
  EventTarget.prototype.addEventListener.call(document,'toggle',function(e){if(e.target instanceof HTMLDetailsElement&&e.target.open)all();},true);
  return 1;
})()
""".trimIndent()
}
