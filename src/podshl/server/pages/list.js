/* One list, paged, filtered and sorted the same way on every page.

   The dashboard grew a "Show 20 more" button when a project with 263
   configurations made it forty-six screens tall, and pressing it eleven times
   is not reading a list: nothing says where you are, the end is never in
   sight, and what you want is not first. The public log was worse — it asked
   for the first thousand entries once and showed them as if that were the log.

   So every list here is this: a page at a time with numbered pages, a filter
   that runs on what is already in this browser, and the orders that make sense
   for that list. The orders are the caller's to choose, and that is not a
   detail: `/projects` may only ever be sorted by host, because any other order
   of a list of projects reads as a ranking.

   Values reach this from strangers' machines and strangers' manifests, so it
   writes text and never markup — the same rule as every page that uses it.

   Served as a file of its own at `/list.js`, and a page that loads it permits
   `'self'` in its script-src; every JSON response carries `nosniff`, so no API
   answer can be loaded as a script in its place. */
(function () {
  "use strict";

  const el = (tag, cls, text) => {
    const n = document.createElement(tag);
    if (cls) n.className = cls;
    if (text !== undefined && text !== null) n.textContent = String(text);
    return n;
  };

  let lists = 0;

  /* 1 … 4 5 6 … 14: the first, the last, and the neighbours of where you are. */
  function pageNumbers(current, total) {
    const want = new Set([1, total, current - 1, current, current + 1]);
    const out = [];
    let last = 0;
    for (let p = 1; p <= total; p++) {
      if (!want.has(p)) continue;
      if (p - last > 1) out.push(null);
      out.push(p);
      last = p;
    }
    return out;
  }

  /* `container` is emptied and filled. Options:

       render(item, index)  a node for one item — a <tr> when `table` is given
       table                column headings, for a list that is a table
       filter               {text(item), label, placeholder, note, initial}
       sorts                [{id, label, compare(a, b)}], the first is the default
       noun                 {one, many}, for "21–40 of 263 configurations"
       empty                what to say when there is nothing at all
       pageSize             20 unless said

     Returns {set(items), query()}. `set` keeps the filter, the order and the
     page size the person chose, and moves back a page only if theirs is gone. */
  window.PodshlList = function (container, o) {
    const id = "plist" + (++lists);
    const sizes = [10, 20, 50, 100];
    const noun = o.noun || {one: "item", many: "items"};
    const sorts = o.sorts || [];
    let items = [];
    let size = o.pageSize || 20;
    let page = 1;
    let sortId = sorts.length ? sorts[0].id : null;
    let query = o.filter && o.filter.initial ? String(o.filter.initial).trim().toLowerCase() : "";

    container.textContent = "";
    const root = el("div", "plist");
    const bar = el("div", "plist-bar");

    let input = null;
    if (o.filter) {
      const wrap = el("div", "plist-filter");
      const label = el("label", null, o.filter.label || "Filter");
      label.htmlFor = id + "-filter";
      input = el("input");
      input.type = "text";
      input.id = id + "-filter";
      input.autocomplete = "off";
      if (o.filter.placeholder) input.placeholder = o.filter.placeholder;
      input.value = o.filter.initial || "";
      input.addEventListener("input", () => {
        query = input.value.trim().toLowerCase();
        page = 1;
        draw();
      });
      wrap.appendChild(label);
      wrap.appendChild(input);
      if (o.filter.note) wrap.appendChild(el("p", "why", o.filter.note));
      bar.appendChild(wrap);
    }

    if (sorts.length > 1) {
      const wrap = el("div", "plist-select");
      const label = el("label", null, "Order");
      label.htmlFor = id + "-sort";
      const select = el("select");
      select.id = id + "-sort";
      sorts.forEach(s => {
        const option = el("option", null, s.label);
        option.value = s.id;
        select.appendChild(option);
      });
      select.addEventListener("change", () => { sortId = select.value; page = 1; draw(); });
      wrap.appendChild(label);
      wrap.appendChild(select);
      bar.appendChild(wrap);
    }

    const sizeWrap = el("div", "plist-select");
    const sizeLabel = el("label", null, "Per page");
    sizeLabel.htmlFor = id + "-size";
    const sizeSelect = el("select");
    sizeSelect.id = id + "-size";
    sizes.forEach(n => {
      const option = el("option", null, n);
      option.value = String(n);
      if (n === size) option.selected = true;
      sizeSelect.appendChild(option);
    });
    sizeSelect.addEventListener("change", () => { size = Number(sizeSelect.value); page = 1; draw(); });
    sizeWrap.appendChild(sizeLabel);
    sizeWrap.appendChild(sizeSelect);
    bar.appendChild(sizeWrap);
    root.appendChild(bar);

    let holder, body;
    if (o.table) {
      holder = el("div", "plist-scroll");
      const table = el("table");
      const head = el("tr");
      o.table.forEach(h => head.appendChild(el("th", null, h)));
      table.appendChild(el("thead")).appendChild(head);
      body = table.appendChild(el("tbody"));
      holder.appendChild(table);
    } else {
      holder = body = el("div", "plist-items");
    }
    root.appendChild(holder);

    const empty = el("p", "why plist-empty");
    root.appendChild(empty);
    const foot = el("div", "plist-foot");
    const info = el("p", "why plist-info");
    info.setAttribute("aria-live", "polite");
    const nav = el("nav", "plist-pages");
    nav.setAttribute("aria-label", "Pages");
    foot.appendChild(info);
    foot.appendChild(nav);
    root.appendChild(foot);
    container.appendChild(root);

    const word = n => n === 1 ? noun.one : noun.many;

    function visible() {
      const list = query && o.filter
        ? items.filter(it => String(o.filter.text(it) || "").toLowerCase().includes(query))
        : items.slice();
      const sort = sorts.find(s => s.id === sortId);
      if (sort && sort.compare) list.sort(sort.compare);
      return list;
    }

    function go(target) {
      page = target;
      draw();
      if (root.getBoundingClientRect().top < 0) root.scrollIntoView({block: "start"});
    }

    function button(label, target, current, disabled) {
      const b = el("button", current ? "current" : null, label);
      b.type = "button";
      if (current) b.setAttribute("aria-current", "page");
      if (label !== String(target)) b.setAttribute("aria-label", label + " page");
      b.disabled = disabled;
      b.addEventListener("click", () => go(target));
      return b;
    }

    function draw() {
      const list = visible();
      const pages = Math.max(1, Math.ceil(list.length / size));
      if (page > pages) page = pages;
      const from = (page - 1) * size;

      body.textContent = "";
      list.slice(from, from + size).forEach((it, i) => body.appendChild(o.render(it, from + i)));

      // A short list needs no controls, unless the person is filtering it.
      bar.hidden = items.length <= sizes[0] && !query;
      const none = list.length === 0;
      holder.hidden = none;
      empty.hidden = !none;
      empty.textContent = !none ? "" : query
        ? "Nothing matches “" + query + "”."
        : (o.empty || "Nothing here yet.");

      info.textContent = none ? "" :
        (list.length <= size
          ? list.length + " " + word(list.length)
          : (from + 1) + "–" + Math.min(from + size, list.length) + " of " +
            list.length + " " + word(list.length)) +
        (query && list.length !== items.length ? " (filtered from " + items.length + ")" : "");

      nav.textContent = "";
      nav.hidden = pages <= 1;
      if (pages > 1) {
        nav.appendChild(button("Previous", page - 1, false, page === 1));
        pageNumbers(page, pages).forEach(p => nav.appendChild(p === null
          ? el("span", "plist-gap", "…")
          : button(String(p), p, p === page, false)));
        nav.appendChild(button("Next", page + 1, false, page === pages));
      }
    }

    draw();
    return {
      set(next) { items = Array.isArray(next) ? next : []; draw(); },
      /* What the person is filtering for, so a list opened from this one can
         start with the same words. */
      query() { return query; },
    };
  };
})();
