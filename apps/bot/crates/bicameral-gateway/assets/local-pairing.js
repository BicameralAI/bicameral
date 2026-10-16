// Bicameral local pairing approval surface (served by the local daemon on
// loopback). Same-origin only: the pending view and its one-time decision
// nonce are readable solely from this origin, and the daemon accepts a
// decision only from a loopback Origin. Bicameral Cloud may open this page
// but cannot read it or submit a decision.
(function () {
  'use strict';
  var API = '/api/v1/hosted-bridge/pair';
  var current = null;

  function $(id) { return document.getElementById(id); }
  function text(id, value) { $(id).textContent = value; }
  function show(id, visible) { $(id).hidden = !visible; }

  function list(id, items) {
    var root = $(id);
    root.replaceChildren();
    (items || []).forEach(function (item) {
      var li = document.createElement('li');
      li.textContent = item;
      root.appendChild(li);
    });
  }

  function expiresLabel(iso) {
    var at = Date.parse(iso);
    if (!isFinite(at)) return 'soon';
    var minutes = Math.max(0, Math.round((at - Date.now()) / 60000));
    return minutes <= 1 ? 'in about a minute' : 'in ' + minutes + ' minutes';
  }

  function finish(title, message, tone) {
    show('review', false);
    show('choose', false);
    text('title', title);
    text('lead', '');
    show('lead', false);
    var result = $('result');
    result.textContent = message;
    result.className = tone || '';
    result.hidden = false;
  }

  function renderView(view) {
    if (view.status !== 'pending') {
      var done = {
        approved: ['Already approved', 'This browser is already connected. You can close this tab.', 'ok'],
        denied: ['Request denied', 'This request was denied. Start again from Bicameral Cloud if that was a mistake.', 'err'],
        expired: ['Request expired', 'This request expired. Return to Bicameral Cloud and choose Try again.', 'err'],
      }[view.status] || ['Request unavailable', 'Return to Bicameral Cloud and start again.', 'err'];
      finish(done[0], done[1], done[2]);
      return;
    }
    current = view;
    text('title', 'Approve this browser?');
    text('lead', 'A browser signed in to Bicameral Cloud wants to connect to Bicameral on this computer. Approve only if you just started this from Bicameral Cloud.');
    text('origin', view.requesting_origin);
    text('account', view.cloud_account_id);
    text('expires', expiresLabel(view.expires_at));
    list('allowed', view.capability_summary && view.capability_summary.allowed);
    list('forbidden', view.capability_summary && view.capability_summary.forbidden);
    show('choose', false);
    show('review', true);
    $('approve').disabled = false;
    $('deny').disabled = false;
  }

  function loadRequest(handle) {
    return fetch(API + '/pending/' + encodeURIComponent(handle), { cache: 'no-store' })
      .then(function (res) {
        if (res.status === 404) { finish('Request not found', 'This request is no longer pending. Return to Bicameral Cloud and choose Try again.', 'err'); return null; }
        if (!res.ok) throw new Error('HTTP ' + res.status);
        return res.json();
      })
      .then(function (view) { if (view) renderView(view); });
  }

  function chooseFrom(requests) {
    if (requests.length === 0) {
      finish('No pending requests', 'Start from Bicameral Cloud in your browser. When it asks for approval, this page will show the request.', '');
      return;
    }
    if (requests.length === 1) {
      selectRequest(requests[0].request_handle);
      return;
    }
    text('title', 'Choose the request to review');
    var root = $('requests');
    root.replaceChildren();
    requests.forEach(function (req) {
      var row = document.createElement('div');
      row.className = 'request';
      var label = document.createElement('span');
      label.textContent = req.requesting_origin + ' · ' + req.cloud_account_id + ' · expires ' + expiresLabel(req.expires_at);
      var button = document.createElement('button');
      button.type = 'button';
      button.className = 'secondary';
      button.textContent = 'Review';
      button.addEventListener('click', function () { selectRequest(req.request_handle); });
      row.appendChild(label);
      row.appendChild(button);
      root.appendChild(row);
    });
    show('choose', true);
  }

  function selectRequest(handle) {
    var url = new URL(window.location.href);
    url.searchParams.set('request', handle);
    window.history.replaceState(null, '', url.toString());
    return loadRequest(handle);
  }

  function decide(action) {
    if (!current) return;
    $('approve').disabled = true;
    $('deny').disabled = true;
    fetch(API + '/decision', {
      method: 'POST',
      cache: 'no-store',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        request_handle: current.request_handle,
        action: action,
        local_confirmation_nonce: current.local_confirmation_nonce,
      }),
    })
      .then(function (res) { return res.json().catch(function () { return {}; }); })
      .then(function (body) {
        current = null;
        if (body.status === 'approved') {
          finish('Browser approved', 'Return to your Bicameral Cloud tab — it continues automatically. You can close this tab.', 'ok');
          window.setTimeout(function () { try { window.close(); } catch (_) {} }, 1500);
        } else if (body.status === 'denied') {
          finish('Request denied', action === 'deny' ? 'Nothing was connected.' : (body.message || 'Approval could not be recorded. Return to Bicameral Cloud and try again.'), 'err');
        } else if (body.status === 'expired') {
          finish('Request expired', 'Return to Bicameral Cloud and choose Try again.', 'err');
        } else {
          finish('Could not record your decision', body.message || 'Return to Bicameral Cloud and try again.', 'err');
        }
      })
      .catch(function () {
        finish('Bicameral stopped responding', 'Make sure Bicameral is running, then return to Bicameral Cloud and try again.', 'err');
      });
  }

  $('approve').addEventListener('click', function () { decide('approve'); });
  $('deny').addEventListener('click', function () { decide('deny'); });

  var handle = new URLSearchParams(window.location.search).get('request');
  var start = handle
    ? loadRequest(handle)
    : fetch(API + '/pending', { cache: 'no-store' })
        .then(function (res) { if (!res.ok) throw new Error('HTTP ' + res.status); return res.json(); })
        .then(function (body) { return chooseFrom(body.requests || []); });
  start.catch(function () {
    finish('Bicameral could not load the request', 'Reload this page. If it keeps failing, return to Bicameral Cloud and choose Try again.', 'err');
  });
})();
