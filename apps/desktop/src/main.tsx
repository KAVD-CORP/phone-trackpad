import React, { useCallback, useEffect, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import './styles.css';

type Pending = { id: number; device_name: string };
type Device = { name: string; key: string };
type Status = {
  service_up: boolean;
  pairing_active: boolean;
  pending: Pending[];
  trusted: Device[];
  live: string | null;
};
type Ceremony = { qr_svg: string; short_code: string; expires_at_ms: number };

function shortKey(key: string): string {
  return key.length > 16 ? `${key.slice(0, 8)}…${key.slice(-8)}` : key;
}

type WebPin = { pin: string; url: string; qr_svg: string; expires_in_secs: number };

function BrowserCard() {
  const [web, setWeb] = useState<WebPin | null>(null);
  const [error, setError] = useState<string>('');
  const show = async () => {
    try {
      const w = await invoke<WebPin>('web_begin');
      setWeb(w);
      setError('');
    } catch (e) {
      setError(String(e));
    }
  };
  return (
    <div>
      <button className="primary" onClick={show}>
        {web ? 'Show a new browser code' : 'Show browser code'}
      </button>
      {error && <p role="alert" style={{ color: 'darkred' }}>{error}</p>}
      {web && (
        <div style={{ marginTop: 12 }}>
          <div className="qr-wrap">
            <img
              alt="Browser pairing QR code"
              width={220}
              height={220}
              src={`data:image/svg+xml;utf8,${encodeURIComponent(web.qr_svg)}`}
            />
            <div>
              <div className="fine">Scan with the iPhone Camera — Safari opens and connects by itself.</div>
              <div className="fine" style={{ marginTop: 8 }}>Or type in Safari:</div>
              <div className="code" style={{ fontSize: 17, letterSpacing: 1 }}>{web.url}</div>
              <div className="fine">Then type this code on the page:</div>
              <div className="code">{web.pin}</div>
            </div>
          </div>
          <div className="fine">Works for about 10 minutes. Add the page to the Home Screen for full screen.</div>
        </div>
      )}
    </div>
  );
}

function App() {
  const [deviceName, setDeviceName] = useState<string>('');
  const [error, setError] = useState<string>('');
  const [status, setStatus] = useState<Status | null>(null);
  const [ceremony, setCeremony] = useState<Ceremony | null>(null);
  const [autostart, setAutostart] = useState<boolean>(false);
  const [bootFailed, setBootFailed] = useState<boolean>(false);
  const [pairingBusy, setPairingBusy] = useState<boolean>(false);

  const refresh = useCallback(async () => {
    try {
      const s = await invoke<Status>('pairing_status');
      setStatus(s);
      setError('');
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const boot = useCallback(async () => {
    try {
      const name = await invoke<string>('boot_service');
      setDeviceName(name);
      setBootFailed(false);
      const auto = await invoke<boolean>('get_autostart');
      setAutostart(auto);
      setError('');
    } catch (e) {
      setBootFailed(true);
      setError(`Service failed to start: ${String(e)}`);
    }
  }, []);

  const startPairing = useCallback(async () => {
    setPairingBusy(true);
    try {
      const c = await invoke<Ceremony>('begin_pairing');
      setCeremony(c);
      setError('');
      refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setPairingBusy(false);
    }
  }, [refresh]);

  const startPairingRef = useRef(startPairing);
  startPairingRef.current = startPairing;

  useEffect(() => {
    boot().finally(refresh);
    const t = setInterval(refresh, 1000);
    const unlisten = listen('open-pairing', () => startPairingRef.current());
    return () => {
      clearInterval(t);
      unlisten.then((f) => f());
    };
  }, [boot, refresh]);

  const decide = async (id: number, ok: boolean) => {
    await invoke('approve_device', { id, ok });
    refresh();
  };

  const revoke = async (key: string) => {
    await invoke('revoke_device', { key });
    refresh();
  };

  return (
    <div className="app">
      <div className="header">
        <div>
          <h1>Phone Trackpad</h1>
          {deviceName && <div className="device">{deviceName}</div>}
        </div>
        <span className={status?.live ? 'pill live' : 'pill'}>
          <span className="dot" />
          {status?.live ? `${status.live} connected` : 'Idle'}
        </span>
      </div>

      {error && (
        <div className="alert" role="alert">
          <div>{error}</div>
          {bootFailed && <button onClick={() => boot().finally(refresh)}>Retry</button>}
        </div>
      )}

      {status && status.pending.length > 0 && (
        <div className="card pending">
          <h2>New phone wants to connect</h2>
          {status.pending.map((p) => (
            <div className="row" key={p.id}>
              <span className="who">
                Allow <strong>{p.device_name}</strong> to control this computer?
              </span>
              <button className="primary" onClick={() => decide(p.id, true)}>
                Allow
              </button>
              <button onClick={() => decide(p.id, false)}>Deny</button>
            </div>
          ))}
        </div>
      )}

      <div className="card">
        <h2>Add a phone</h2>
        <p className="desc">Scan with the phone app - nothing leaves your Wi-Fi.</p>
        <p className="fine">Use the scanner inside the phone app — the iPhone Camera app can't read this code.</p>
        <button className="primary" onClick={startPairing} disabled={pairingBusy}>
          {pairingBusy ? 'Preparing…' : ceremony ? 'Show a new code' : 'Show pairing code'}
        </button>
        {ceremony && (
          <div className="qr-wrap" style={{ marginTop: 14 }}>
            <img
              alt="Pairing QR code"
              width={220}
              height={220}
              src={`data:image/svg+xml;utf8,${encodeURIComponent(ceremony.qr_svg)}`}
            />
            <div>
              <div className="fine">No camera? Type this on the phone:</div>
              <div className="code">{ceremony.short_code}</div>
              <div className="fine">Expires after about 2 minutes.</div>
            </div>
          </div>
        )}
      </div>

      <div className="card">
        <h2>Trusted phones</h2>
        {status && status.trusted.length > 0 ? (
          <ul className="devices">
            {status.trusted.map((d) => (
              <li key={d.key}>
                <span>
                  <span className="name">{d.name}</span> <code>{shortKey(d.key)}</code>
                </span>
                <button className="danger-ghost" onClick={() => revoke(d.key)}>
                  Revoke
                </button>
              </li>
            ))}
          </ul>
        ) : (
          <p className="empty">No phones yet — pair one above.</p>
        )}
      </div>

      <div className="card">
        <h2>No app? Use the browser</h2>
        <p className="desc">
          On the iPhone, open Safari and type the address. No install needed — trusted Wi-Fi only.
        </p>
        <BrowserCard />
      </div>

      <div className="card">
        <h2>Settings</h2>
        <label className="switch-row">
          <input
            type="checkbox"
            checked={autostart}
            onChange={async (e) => {
              const enabled = e.target.checked;
              try {
                await invoke('set_autostart', { enabled });
                setAutostart(enabled);
              } catch (err) {
                setError(String(err));
              }
            }}
          />
          <span>Start on login</span>
        </label>
      </div>
    </div>
  );
}

const el = document.getElementById('root');
if (el) {
  createRoot(el).render(<App />);
}
