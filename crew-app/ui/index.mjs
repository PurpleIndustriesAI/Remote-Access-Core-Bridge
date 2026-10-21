const modules = window.__codexcrew_modules
const React = modules.react
const { createElement: h, useCallback, useEffect, useRef, useState } = React
const { useAppApi } = modules['@codexcrew/app-sdk']
const BASE = '/api/apps/remote-access-crew/rac'

const css = `
.rac{color:var(--text);background:var(--bg);padding:24px;min-height:100%;max-width:1200px;margin:auto;font:inherit}
.rac *{box-sizing:border-box}
.rac header{display:flex;gap:16px;align-items:center;justify-content:space-between;flex-wrap:wrap;margin-bottom:20px}
.rac .brand{display:flex;gap:12px;align-items:center}
.rac h1{font-size:22px;margin:0;font-weight:600}
.rac h2{font-size:13px;margin:0 0 12px;font-weight:600;color:var(--accent,#7c3aed);letter-spacing:.01em}
.rac p,.rac small{color:var(--muted);line-height:1.55}
.rac small{display:block;font-size:12px}
.rac .grid{display:grid;grid-template-columns:minmax(0,5fr) minmax(0,7fr);gap:16px}
.rac .card{background:var(--card,var(--bg));border:1px solid var(--border);border-radius:8px;padding:16px;margin-bottom:16px}
.rac button,.rac input,.rac textarea,.rac select{font:inherit;font-size:13px;color:var(--text);background:var(--bg);border:1px solid var(--border);border-radius:6px;padding:8px 11px}
.rac button{cursor:pointer;min-height:36px;white-space:nowrap}
.rac button:hover:not(:disabled){border-color:var(--accent,#7c3aed)}
.rac button.primary{background:var(--accent,#7c3aed);border-color:var(--accent,#7c3aed);color:var(--accent-text,#fff)}
.rac button.danger{color:var(--danger,#b91c1c)}
.rac button:disabled{opacity:.5;cursor:not-allowed}
.rac :focus-visible{outline:2px solid var(--accent,#7c3aed);outline-offset:2px}
.rac .row{display:flex;gap:8px;align-items:center;flex-wrap:wrap}
.rac .spread{justify-content:space-between}
.rac .chip{display:inline-block;padding:2px 8px;border-radius:9999px;font-size:11px;font-weight:600;border:1px solid var(--border);color:var(--muted)}
.rac .chip.ok{color:#047857;border-color:#04785755}
.rac .chip.warn{color:#b45309;border-color:#b4530955}
.rac .chip.ts{color:var(--accent,#7c3aed);border-color:var(--accent,#7c3aed)}
.rac .peer{border-top:1px solid var(--border);padding:12px 0}
.rac .peer:first-of-type{border-top:0;padding-top:0}
.rac .name{font-weight:600;font-size:14px}
.rac code{font-size:12px;overflow-wrap:anywhere}
.rac .code{font:600 26px/1.2 ui-monospace,Consolas,monospace;letter-spacing:.06em;margin:6px 0}
.rac label{display:flex;flex-direction:column;gap:6px;font-size:12px;color:var(--muted);flex:1;min-width:150px}
.rac textarea{width:100%;min-height:84px;resize:vertical}
.rac .paste{border:1px dashed var(--border);border-radius:8px;padding:14px;text-align:center;color:var(--muted);font-size:12px;margin:10px 0;outline:none}
.rac .paste:focus{border-color:var(--accent,#7c3aed);color:var(--text)}
.rac .paste img{max-width:100%;max-height:160px;display:block;margin:8px auto 0;border-radius:4px}
.rac .ev{display:grid;grid-template-columns:62px 22px minmax(0,1fr) auto;gap:8px;padding:7px 0;border-top:1px solid var(--border);font-size:12px;align-items:baseline}
.rac .ev .fail{color:var(--danger,#b91c1c)}
.rac .alert{border:1px solid var(--danger,#b91c1c);border-radius:6px;padding:10px 12px;margin-bottom:12px}
.rac .notice{border:1px solid var(--border);border-radius:6px;padding:10px 12px;margin-bottom:12px}
.rac .switch{display:inline-flex;align-items:center;gap:6px;font-size:12px;cursor:pointer;color:var(--muted);flex:none;min-width:0;flex-direction:row}
@media(max-width:820px){.rac .grid{grid-template-columns:1fr}.rac{padding:16px}}
`

function Logo({ size = 28 }) {
  return h('svg', { width: size, height: size, viewBox: '0 0 64 64', 'aria-hidden': true },
    h('rect', { x: 2, y: 2, width: 60, height: 60, rx: 14, fill: '#2a1b4a' }),
    h('rect', { x: 25, y: 11, width: 28, height: 19, rx: 3, fill: 'none', stroke: '#c4b5fd', strokeWidth: 3.2 }),
    h('path', { d: 'M39 30v4M34 35h10', stroke: '#c4b5fd', strokeWidth: 3.2, strokeLinecap: 'round' }),
    h('rect', { x: 11, y: 27, width: 28, height: 19, rx: 3, fill: '#2a1b4a', stroke: '#a78bfa', strokeWidth: 3.2 }),
    h('path', { d: 'M25 46v4M20 51h10', stroke: '#a78bfa', strokeWidth: 3.2, strokeLinecap: 'round' }),
    h('path', { d: 'M17 37h16M21 33l-4 4 4 4M29 33l4 4-4 4', fill: 'none', stroke: '#f5f3ff', strokeWidth: 2.6, strokeLinecap: 'round', strokeLinejoin: 'round' }))
}

const isTs = a => /^100\.(6[4-9]|[7-9]\d|1[01]\d|12[0-7])\./.test(a) || a.startsWith('[fd7a:115c:a1e0')
const bytes = n => n < 1024 ? `${n} B` : n < 1048576 ? `${(n / 1024).toFixed(1)} KB` : n < 1073741824 ? `${(n / 1048576).toFixed(1)} MB` : `${(n / 1073741824).toFixed(2)} GB`
const time = ts => new Date(ts * 1000).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })

function toPngBase64(file) {
  return new Promise((resolve, reject) => {
    const url = URL.createObjectURL(file)
    const img = new Image()
    img.onload = () => {
      const c = document.createElement('canvas')
      c.width = img.naturalWidth; c.height = img.naturalHeight
      c.getContext('2d').drawImage(img, 0, 0)
      URL.revokeObjectURL(url)
      resolve(c.toDataURL('image/png').split(',')[1])
    }
    img.onerror = () => { URL.revokeObjectURL(url); reject(new Error('That image could not be read.')) }
    img.src = url
  })
}

export default function RemoteAccessCrew() {
  const api = useAppApi()
  const [data, setData] = useState(null)
  const [bridge, setBridge] = useState('loading')
  const [error, setError] = useState('')
  const [notice, setNotice] = useState('')
  const [busy, setBusy] = useState('')
  const [target, setTarget] = useState('')
  const [text, setText] = useState('')
  const [image, setImage] = useState(null)
  const [paths, setPaths] = useState('')
  const [pairAddr, setPairAddr] = useState('')
  const [pairCode, setPairCode] = useState('')
  const [confirmRemove, setConfirmRemove] = useState('')
  const [clock, setClock] = useState(Date.now())
  const fetchedAt = useRef(Date.now())

  const refresh = useCallback(async () => {
    try {
      const r = await api.get(`${BASE}/status`)
      setBridge(r.bridge)
      if (r.result) { setData(r.result); fetchedAt.current = Date.now() }
      setError('')
    } catch (e) { setBridge('down'); setError(e.message) }
  }, [api])

  useEffect(() => { refresh(); const t = setInterval(refresh, 3000); return () => clearInterval(t) }, [refresh])
  useEffect(() => { const t = setInterval(() => setClock(Date.now()), 1000); return () => clearInterval(t) }, [])

  async function act(path, body, done) {
    setBusy(path); setError(''); setNotice('')
    try {
      const r = await api.post(`${BASE}/${path}`, body || {})
      const results = r.result?.results
      if (results) {
        const bad = results.filter(x => !x.ok)
        if (bad.length) setError(bad.map(x => `${x.peer}: ${x.error}`).join(' · '))
        else setNotice(`Delivered to ${results.map(x => x.peer).join(', ')}.`)
      } else if (done) setNotice(done(r.result))
      await refresh()
      return r.result
    } catch (e) { setError(e.message) } finally { setBusy('') }
  }

  async function onPaste(e) {
    const items = [...(e.clipboardData?.items || [])]
    const img = items.find(i => i.type.startsWith('image/'))
    if (img) {
      e.preventDefault()
      try {
        const file = img.getAsFile()
        setImage({ b64: await toPngBase64(file), preview: URL.createObjectURL(file), size: file.size })
      } catch (err) { setError(err.message) }
      return
    }
    const t = e.clipboardData?.getData('text/plain')
    if (t) { e.preventDefault(); setText(t) }
  }

  const peers = data?.peers || []
  const pairing = data?.pairing?.open ? data.pairing : null
  const left = pairing ? Math.max(0, pairing.expires_in - Math.floor((clock - fetchedAt.current) / 1000)) : 0
  const sendTo = target ? { peer: target } : {}
  const syncOn = data?.settings?.autosync
  const disabled = !!busy || bridge !== 'running'

  const header = h('header', null,
    h('div', { className: 'brand' }, h(Logo, { size: 34 }),
      h('div', null, h('h1', null, 'Remote Access Crew'),
        h('small', null, 'Copy on one machine, paste on the other. Text, images, and files over your LAN or Tailscale.'))),
    h('div', { className: 'row' },
      h('span', { className: `chip ${bridge === 'running' ? 'ok' : 'warn'}` }, bridge === 'running' ? 'Bridge running' : bridge === 'missing' ? 'Bridge not installed' : bridge === 'loading' ? 'Checking…' : 'Bridge stopped'),
      data && h('button', { disabled, onClick: () => act('set', { autosync: !syncOn }), 'aria-pressed': !!syncOn },
        syncOn ? 'Automatic sync: on' : 'Automatic sync: off'),
      data && h('span', { className: 'chip' }, `v${data.version}`)))

  if (bridge === 'missing') {
    return h('main', { className: 'rac' }, h('style', null, css), header,
      h('div', { className: 'card' }, h('h2', null, 'Install the bridge'),
        h('p', null, 'The Remote Access Crew bridge is not installed in this app yet. Run the app installer from the release (install.ps1 on Windows, install.sh on Linux), then reopen this page.')))
  }

  const thisMachine = h('div', { className: 'card' },
    h('h2', null, 'This machine'),
    !data ? h('p', null, 'Loading…') : h(React.Fragment, null,
      h('div', { className: 'name' }, data.name),
      h('small', null, 'Identity ', h('code', null, data.short_fingerprint)),
      h('div', { className: 'row', style: { marginTop: 8 } }, ...(data.addrs || []).map(a => h('span', { key: a, className: `chip ${isTs(a) ? 'ts' : ''}` }, `${isTs(a) ? 'Tailscale' : 'LAN'} ${a}`))),
      h('small', { style: { marginTop: 8 } }, 'Received files go to ', h('code', null, data.receive_dir || '…'))))

  const peerList = h('div', { className: 'card' },
    h('h2', null, `Paired devices (${peers.length})`),
    !peers.length ? h('p', null, 'No devices yet. Pair one below.') : peers.map(p => h('div', { key: p.fingerprint, className: 'peer' },
      h('div', { className: 'row spread' },
        h('div', null, h('div', { className: 'name' }, p.name), h('small', null, h('code', null, p.short_fingerprint))),
        h('label', { className: 'switch' }, h('input', { type: 'checkbox', checked: !!p.autosync, disabled, onChange: e => act('set', { autosync: e.target.checked, peer: p.fingerprint }) }), 'Sync')),
      h('div', { className: 'row', style: { margin: '6px 0' } }, ...p.addrs.slice(0, 4).map(a => h('span', { key: a, className: `chip ${isTs(a) ? 'ts' : ''}` }, `${isTs(a) ? 'Tailscale' : 'LAN'} ${a}`))),
      h('div', { className: 'row' },
        h('button', { disabled, onClick: () => act('ping', { peer: p.fingerprint }, () => `${p.name} is reachable and trusted.`) }, busy === 'ping' ? 'Testing…' : 'Test connection'),
        confirmRemove === p.fingerprint
          ? h(React.Fragment, null, h('button', { className: 'danger', disabled, onClick: () => { setConfirmRemove(''); act('unpair', { peer: p.fingerprint }, () => `${p.name} removed. It can no longer connect.`) } }, 'Confirm remove'), h('button', { onClick: () => setConfirmRemove('') }, 'Keep'))
          : h('button', { className: 'danger', disabled, onClick: () => setConfirmRemove(p.fingerprint) }, 'Remove')))))

  const pairCard = h('div', { className: 'card' },
    h('h2', null, 'Pair a device'),
    pairing
      ? h(React.Fragment, null,
        h('p', null, 'On the other machine, open Remote Access Crew and choose “Enter a code”, or run ', h('code', null, 'rac pair <address> <code>'), '.'),
        h('div', { className: 'code', 'aria-live': 'polite' }, pairing.code),
        h('small', null, `Expires in ${Math.floor(left / 60)}:${String(left % 60).padStart(2, '0')}. One use. Five wrong attempts close it.`),
        h('div', { className: 'row', style: { margin: '8px 0' } }, ...(data.addrs || []).map(a => h('span', { key: a, className: `chip ${isTs(a) ? 'ts' : ''}` }, a))),
        h('button', { disabled, onClick: () => act('pair-cancel') }, 'Cancel pairing'))
      : h(React.Fragment, null,
        h('div', { className: 'row' }, h('button', { className: 'primary', disabled, onClick: () => act('pair-code') }, 'Show a pairing code')),
        h('p', { style: { margin: '14px 0 8px' } }, 'Or enter the code shown on the other machine:'),
        h('form', { className: 'row', onSubmit: e => { e.preventDefault(); act('pair', { addr: pairAddr.trim(), code: pairCode.trim() }, r => `Paired with ${r.name} (${r.short_fingerprint}).`).then(r => { if (r) { setPairCode(''); setPairAddr('') } }) } },
          h('label', null, 'Address (LAN or Tailscale)', h('input', { value: pairAddr, onChange: e => setPairAddr(e.target.value), placeholder: '192.168.1.20 or 100.x.y.z', autoComplete: 'off' })),
          h('label', null, 'Code', h('input', { value: pairCode, onChange: e => setPairCode(e.target.value), placeholder: 'XXXX-XXXX-XXXX-XXXX', autoComplete: 'off', spellCheck: false })),
          h('button', { type: 'submit', disabled: disabled || !pairAddr.trim() || pairCode.replace(/[^a-z0-9]/gi, '').length !== 16, style: { alignSelf: 'flex-end' } }, busy === 'pair' ? 'Pairing…' : 'Pair'))),
    h('small', { style: { marginTop: 10 } }, 'Pairing pins each machine’s certificate. Only pinned machines on your LAN or Tailscale can connect.'))

  const sendCard = h('div', { className: 'card' },
    h('div', { className: 'row spread' }, h('h2', { style: { margin: 0 } }, 'Send'),
      h('select', { value: target, onChange: e => setTarget(e.target.value), 'aria-label': 'Send to' },
        h('option', { value: '' }, 'All devices'), ...peers.map(p => h('option', { key: p.fingerprint, value: p.fingerprint }, p.name)))),
    h('p', { style: { marginTop: 8 } }, syncOn ? 'Automatic sync is on: anything you copy is already sent to devices with Sync checked. Use this panel to send on demand.' : 'Automatic sync is off. Send items on demand here.'),
    h('div', { className: 'row' }, h('button', { className: 'primary', disabled: disabled || !peers.length, onClick: () => act('send', { ...sendTo }) }, busy === 'send' ? 'Sending…' : 'Send what I copied')),
    h('div', { className: 'paste', tabIndex: 0, onPaste, role: 'textbox', 'aria-label': 'Paste an image or text here' },
      image ? h(React.Fragment, null, `Image ready (${bytes(image.size)})`, h('img', { src: image.preview, alt: 'Pasted image preview' })) : 'Click here and paste (Ctrl+V) an image or text'),
    image && h('div', { className: 'row', style: { marginBottom: 10 } },
      h('button', { className: 'primary', disabled: disabled || !peers.length, onClick: () => act('send', { ...sendTo, image_png_b64: image.b64 }).then(() => setImage(null)) }, 'Send image'),
      h('button', { onClick: () => setImage(null) }, 'Clear')),
    h('label', null, 'Text', h('textarea', { value: text, onChange: e => setText(e.target.value), placeholder: 'Type or paste text to send', maxLength: 2000000 })),
    h('div', { className: 'row', style: { margin: '8px 0 14px' } }, h('button', { disabled: disabled || !peers.length || !text, onClick: () => act('send', { ...sendTo, text }).then(() => setText('')) }, 'Send text')),
    h('label', null, 'Files or folders on this machine (one path per line)', h('textarea', { value: paths, onChange: e => setPaths(e.target.value), placeholder: 'C:\\Users\\you\\Documents\\report.pdf', style: { minHeight: 60 } })),
    h('div', { className: 'row', style: { marginTop: 8 } }, h('button', { disabled: disabled || !peers.length || !paths.trim(), onClick: () => act('send', { ...sendTo, paths: paths.split('\n').map(s => s.trim()).filter(Boolean) }).then(() => setPaths('')) }, 'Send files')))

  const events = data?.events || []
  const activity = h('div', { className: 'card' },
    h('h2', null, 'Activity'),
    !events.length ? h('p', null, 'Nothing sent or received yet. Content is never logged; only type and size.') :
      events.slice(0, 40).map((e, i) => h('div', { key: i, className: 'ev' },
        h('span', null, time(e.ts)),
        h('span', { 'aria-label': e.dir === 'in' ? 'received' : 'sent' }, e.dir === 'in' ? '↓' : '↑'),
        h('span', null, h('strong', null, e.peer), ` ${e.kind}${e.bytes ? ` · ${bytes(e.bytes)}` : ''}`, h('small', { className: e.ok ? '' : 'fail' }, e.detail)),
        h('span', { className: `chip ${e.ok ? 'ok' : 'warn'}` }, e.ok ? 'OK' : 'Failed'))))

  return h('main', { className: 'rac' }, h('style', null, css), header,
    error && h('div', { className: 'alert', role: 'alert' }, error),
    notice && h('div', { className: 'notice', role: 'status' }, notice),
    h('div', { className: 'grid' },
      h('div', null, thisMachine, peerList, pairCard),
      h('div', null, sendCard, activity)))
}
