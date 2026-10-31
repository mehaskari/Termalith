import { type FormEvent, useEffect, useState } from "react";
import { type SerialConfig, type SerialPortInfo, isIpcError, serialPorts } from "../terminal/api";
import { Dialog } from "./Dialog";

const BAUD_RATES = [1200, 2400, 4800, 9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600];

interface Props {
  onOpen: (config: SerialConfig) => void;
  onCancel: () => void;
}

export function SerialDialog({ onOpen, onCancel }: Props) {
  const [ports, setPorts] = useState<SerialPortInfo[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [c, setC] = useState<SerialConfig>({
    path: "",
    baudRate: 9600,
    dataBits: 8,
    parity: "none",
    stopBits: 1,
    flowControl: "none",
  });
  const set = (patch: Partial<SerialConfig>) => setC((x) => ({ ...x, ...patch }));

  const refresh = () => {
    setError(null);
    serialPorts().then(
      (p) => {
        setPorts(p);
        setC((x) => (x.path || !p[0] ? x : { ...x, path: p[0].path }));
      },
      (e) => setError(isIpcError(e) ? e.message : String(e)),
    );
  };
  useEffect(refresh, []);

  function submit(e: FormEvent) {
    e.preventDefault();
    if (!c.path.trim()) return setError("Choose or type a serial port");
    if (!Number.isInteger(c.baudRate) || c.baudRate < 50) return setError("Invalid baud rate");
    onOpen({ ...c, path: c.path.trim() });
  }

  return (
    <Dialog
      title="Serial console"
      onCancel={onCancel}
      actions={
        <>
          <button type="button" onClick={onCancel}>
            Cancel
          </button>
          <button type="submit" form="serial-form" className="primary">
            Open
          </button>
        </>
      }
    >
      <form id="serial-form" className="form-grid" onSubmit={submit}>
        <label className="span-2">
          Port
          <span className="row">
            <input
              list="serial-ports"
              value={c.path}
              placeholder={ports?.length === 0 ? "No ports found; type a device path" : "/dev/ttyUSB0 or COM3"}
              spellCheck={false}
              onChange={(e) => set({ path: e.target.value })}
            />
            <button type="button" onClick={refresh} title="Rescan ports">
              ⟳
            </button>
          </span>
          <datalist id="serial-ports">
            {ports?.map((p) => (
              <option key={p.path} value={p.path}>
                {p.description}
              </option>
            ))}
          </datalist>
        </label>
        <label>
          Baud rate
          <select value={c.baudRate} onChange={(e) => set({ baudRate: Number(e.target.value) })}>
            {BAUD_RATES.map((b) => (
              <option key={b} value={b}>
                {b}
              </option>
            ))}
          </select>
        </label>
        <label>
          Data bits
          <select value={c.dataBits} onChange={(e) => set({ dataBits: Number(e.target.value) })}>
            {[8, 7, 6, 5].map((d) => (
              <option key={d} value={d}>
                {d}
              </option>
            ))}
          </select>
        </label>
        <label>
          Parity
          <select value={c.parity} onChange={(e) => set({ parity: e.target.value as SerialConfig["parity"] })}>
            <option value="none">None</option>
            <option value="even">Even</option>
            <option value="odd">Odd</option>
          </select>
        </label>
        <label>
          Stop bits
          <select value={c.stopBits} onChange={(e) => set({ stopBits: Number(e.target.value) })}>
            <option value={1}>1</option>
            <option value={2}>2</option>
          </select>
        </label>
        <label className="span-2">
          Flow control
          <select value={c.flowControl} onChange={(e) => set({ flowControl: e.target.value as SerialConfig["flowControl"] })}>
            <option value="none">None</option>
            <option value="software">Software (XON/XOFF)</option>
            <option value="hardware">Hardware (RTS/CTS)</option>
          </select>
        </label>
        {error && <p className="form-error span-2" role="alert">{error}</p>}
      </form>
    </Dialog>
  );
}
