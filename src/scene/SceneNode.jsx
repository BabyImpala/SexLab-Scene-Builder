import { useEffect, useRef } from 'react';
import Icon, { EditOutlined, CopyOutlined, CloseOutlined, WarningOutlined, ArrowRightOutlined, HeartFilled } from '@ant-design/icons';
import { register } from "@antv/x6-react-shape";
import './SceneNode.css'

const NODE_HEIGHT = 112;
const NODE_WIDTH = 240;
const START_COLOR = 'rgb(0, 88, 0)';
const PORT_DEFAULTS = {
  fill: 'rgb(201, 225, 195, 0.3)',
  stroke: 'black',
}

function makeColor(r, g, b, a = 1) {
  return `rgba(${r}, ${g}, ${b}, ${a})`;
}

function FixedLength(props) {
  const fixedLen_svg = () => (
    <svg viewBox="112 176 800 672" width="1em" height="1em" fill="currentColor">
      <path d="M 180 176 h -60 c -4.4 0 -8 3.6 -8 8 v 656 c 0 4.4 3.6 8 8 8 h 60 c 4.4 0 8 -3.6 8 -8 V 184 c 0 -4.4 -3.6 -8 -8 -8 z m 724 0 h -60 c -4.4 0 -8 3.6 -8 8 v 656 c 0 4.4 3.6 8 8 8 h 60 c 4.4 0 8 -3.6 8 -8 V 184 c 0 -4.4 -3.6 -8 -8 -8 z M 785.3 504.3 L 657.7 403.6 a 7.23 7.23 0 0 0 -11.7 5.7 V 476 H 238 V 548 h 407.3 v 62.8 c 0 6 7 9.4 11.7 5.7 l 127.5 -100.8 c 3.8 -2.9 3.8 -8.5 0.2 -11.4 z" />
    </svg>
  );
  return (
    <Icon component={fixedLen_svg} {...props} />
  )
}

/** 2nd icon mouseenter never fires in X6 FO — hit-test on mousemove instead. */
function StatusIconRow({ items }) {
  const tipElRef = useRef(null);
  const rowRef = useRef(null);

  useEffect(() => () => {
    tipElRef.current?.remove();
    tipElRef.current = null;
  }, []);

  if (!items.length) return null;

  const hideTip = () => {
    if (tipElRef.current) tipElRef.current.style.display = 'none';
  };

  const showTip = (title, el) => {
    const r = el.getBoundingClientRect();
    let tipEl = tipElRef.current;
    if (!tipEl) {
      tipEl = window.document.createElement('div');
      tipEl.className = 'node-status-floating-tip';
      tipEl.setAttribute('role', 'tooltip');
      window.document.body.appendChild(tipEl);
      tipElRef.current = tipEl;
    }
    tipEl.textContent = title;
    tipEl.style.left = `${r.left + r.width / 2}px`;
    tipEl.style.top = `${r.top}px`;
    tipEl.style.display = 'block';
  };

  const updateFromPoint = (clientX, clientY) => {
    const row = rowRef.current;
    if (!row) return;
    const spans = row.querySelectorAll('.node-status-icon');
    for (const s of spans) {
      const r = s.getBoundingClientRect();
      if (clientX >= r.left && clientX <= r.right && clientY >= r.top && clientY <= r.bottom) {
        showTip(s.getAttribute('aria-label'), s);
        return;
      }
    }
  };

  return (
    <div
      ref={rowRef}
      className="node-attribute-icons"
      onMouseLeave={hideTip}
      onMouseMove={(e) => updateFromPoint(e.clientX, e.clientY)}
    >
      {items.map(({ title, icon }) => (
        <span
          key={title}
          className="node-status-icon"
          aria-label={title}
        >
          {icon}
        </span>
      ))}
    </div>
  );
}

function NodeCtrlBtn({ label, onClick, danger, children }) {
  const tipElRef = useRef(null);
  useEffect(() => () => {
    tipElRef.current?.remove();
    tipElRef.current = null;
  }, []);

  const hideTip = () => {
    if (tipElRef.current) tipElRef.current.style.display = 'none';
  };
  const showTip = (e) => {
    const r = e.currentTarget.getBoundingClientRect();
    let tipEl = tipElRef.current;
    if (!tipEl) {
      tipEl = window.document.createElement('div');
      tipEl.className = 'node-status-floating-tip';
      tipEl.setAttribute('role', 'tooltip');
      window.document.body.appendChild(tipEl);
      tipElRef.current = tipEl;
    }
    tipEl.textContent = label;
    tipEl.style.left = `${r.left + r.width / 2}px`;
    tipEl.style.top = `${r.top}px`;
    tipEl.style.display = 'block';
  };

  return (
    <button
      type="button"
      className={`node-ctrl-btn${danger ? ' node-ctrl-btn-danger' : ''}`}
      aria-label={label}
      onClick={(e) => {
        e.stopPropagation();
        onClick();
      }}
      onMouseEnter={showTip}
      onMouseLeave={hideTip}
    >
      {children}
    </button>
  );
}

function StageNode({ node, graph }) {
  const stage = node.prop('stage');
  const start = node.prop('isStart');
  const fixedLen = node.prop('fixedLen');

  const label = stage.name;
  const navText = stage.extra.nav_text;
  const orgasm =
    !!node.prop('isOrgasm') ||
    !!(stage.positions && stage.positions.some((pos) => pos.climax || pos.extra?.climax));
  const color = fixedLen ?
    fixedLen < 50 ? makeColor(255, 175, 175, 1) :
      makeColor(175, 235, 255, 1)
    : undefined;

  // Mutating ports during render desyncs edge anchors on WebKitGTK.
  useEffect(() => {
    node.prop('ports/groups/out/attrs/path/stroke', start ? START_COLOR : PORT_DEFAULTS.stroke);
    node.prop('ports/groups/out/attrs/path/fill', color ? color : PORT_DEFAULTS.fill);
  }, [node, start, color]);

  const editStage = () => graph.emit("node:edit", { node });
  const cloneStage = () => graph.emit("node:clone", { node });
  const cloneStageTo = () => graph.emit("node:cloneTo", { node });

  return (
    <div
      className="stage-content"
      style={{
        backgroundColor: color,
        borderColor: start ? START_COLOR : undefined,
      }}
    >
      <div className="node-header">
        <StatusIconRow
          items={[
            start
              ? {
                  title: 'Start Animation',
                  icon: <ArrowRightOutlined style={{ fontSize: 20, color: makeColor(17, 175, 17) }} />,
                }
              : null,
            orgasm
              ? {
                  title: 'Orgasm Stage',
                  icon: <HeartFilled style={{ fontSize: 20, color: makeColor(255, 20, 147) }} />,
                }
              : null,
            !navText && !start
              ? {
                  title: 'Missing navigation text',
                  icon: <WarningOutlined style={{ fontSize: 20, color: makeColor(255, 0, 0) }} />,
                }
              : null,
            fixedLen
              ? {
                  title: 'Fixed Length',
                  icon: <FixedLength style={{ fontSize: 20, color: makeColor(0, 191, 255) }} />,
                }
              : null,
          ].filter(Boolean)}
        />
        <div className="node-controll-button-holder">
          <NodeCtrlBtn label="Edit" onClick={editStage}>
            <EditOutlined />
          </NodeCtrlBtn>
          <NodeCtrlBtn label="Clone" onClick={cloneStage}>
            <CopyOutlined />
          </NodeCtrlBtn>
          <NodeCtrlBtn label="Clone to…" onClick={cloneStageTo}>
            <CopyOutlined />
          </NodeCtrlBtn>
          <NodeCtrlBtn label="Mark as root" onClick={() => graph.emit("node:doMarkRoot", { node })}>
            Root
          </NodeCtrlBtn>
          <NodeCtrlBtn label="Delete" danger onClick={() => node.remove()}>
            <CloseOutlined />
          </NodeCtrlBtn>
        </div>
      </div>
      <div className="stage-name">
        <h4 title={label || 'Untitled'}>{label || 'Untitled'}</h4>
      </div>
    </div>
  );
}

register({
  shape: "stage_node",
  width: NODE_WIDTH,
  height: NODE_HEIGHT,
  ports: {
    groups: {
      out: {
        markup: [{ tagName: 'path', selector: 'path' }],
        attrs: {
          path: {
            d: 'M 0 -40 L 10 0 L 0 40 z',
            magnet: true,
            stroke: PORT_DEFAULTS.stroke,
            strokeWidth: 1,
            fill: PORT_DEFAULTS.fill,
          },
        },
        position: { name: 'absolute' },
      },
      in: {
        markup: [{ tagName: 'circle', selector: 'circle' }],
        attrs: {
          circle: {
            r: 4,
            magnet: true,
            stroke: 'transparent',
            fill: 'transparent',
          },
        },
        position: { name: 'absolute' },
      },
    },
    items: [
      {
        id: 'out',
        group: 'out',
        args: { x: NODE_WIDTH - 1, y: NODE_HEIGHT / 2 },
      },
      {
        id: 'in',
        group: 'in',
        args: { x: 0, y: NODE_HEIGHT / 2 },
      },
    ],
  },
  effect: ['name', 'stage', 'scene', 'isOrgasm', 'fixedLen', 'isStart'],
  component: StageNode,
});

export { NODE_WIDTH, NODE_HEIGHT };
