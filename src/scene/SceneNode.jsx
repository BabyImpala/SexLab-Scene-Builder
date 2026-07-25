import { useEffect, useRef } from 'react';
import Icon, { EditOutlined, CopyOutlined, CloseOutlined, WarningOutlined, ArrowRightOutlined, HeartFilled } from '@ant-design/icons';
import { register } from "@antv/x6-react-shape";
import { uniqueStageLabel } from './stageFamily';
import './SceneNode.css'

const NODE_HEIGHT = 112;
const NODE_WIDTH = 240;
const START_COLOR = 'rgb(0, 88, 0)';

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
  const stage = node.prop('stage') || {};
  const start = node.prop('isStart');
  const fixedLen = node.prop('fixedLen');
  const isTransition = !!node.prop('isTransition');
  const hubReturns = Number(node.prop('hubReturns') || 0);
  const poseFamilyLabel = node.prop('poseFamily');
  const scene = node.prop('scene') || {};

  const label = uniqueStageLabel(stage, scene.stages || []) || stage.name;
  const navText = stage.extra?.nav_text;
  const orgasm =
    !!node.prop('isOrgasm') ||
    !!(stage.positions && stage.positions.some((pos) => pos.climax || pos.extra?.climax));
  const color = isTransition
    ? makeColor(196, 155, 90, 1)
    : fixedLen
      ? fixedLen < 50
        ? makeColor(255, 175, 175, 1)
        : makeColor(175, 235, 255, 1)
      : undefined;

  const editStage = () => graph.emit("node:edit", { node });
  const cloneStage = () => graph.emit("node:clone", { node });
  const cloneStageTo = () => graph.emit("node:cloneTo", { node });

  return (
    <div
      className={`stage-content${isTransition ? ' stage-transition' : ''}`}
      style={{
        backgroundColor: color,
        borderColor: start ? START_COLOR : isTransition ? 'rgba(120, 80, 20, 0.55)' : undefined,
      }}
    >
      <div className="node-header">
        <StatusIconRow
          items={[
            isTransition
              ? {
                  title: 'Transition stage',
                  icon: (
                    <span style={{ fontSize: 10, fontWeight: 700, color: makeColor(90, 55, 10) }}>
                      T
                    </span>
                  ),
                }
              : null,
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
            !navText && !start && !isTransition
              ? {
                  title: 'Missing navigation text',
                  icon: <WarningOutlined style={{ fontSize: 20, color: makeColor(255, 0, 0) }} />,
                }
              : null,
            fixedLen && !isTransition
              ? {
                  title: 'Fixed Length',
                  icon: <FixedLength style={{ fontSize: 20, color: makeColor(0, 191, 255) }} />,
                }
              : null,
            hubReturns > 0
              ? {
                  title: `${hubReturns} cross-family return(s) into this hub`,
                  icon: (
                    <span style={{ fontSize: 12, fontWeight: 700, color: makeColor(194, 65, 12) }}>
                      ←{hubReturns}
                    </span>
                  ),
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
      {poseFamilyLabel && !isTransition ? (
        <div style={{ fontSize: 10, opacity: 0.55, padding: '0 8px', marginTop: -2 }}>
          {poseFamilyLabel}
        </div>
      ) : null}
      <div className="stage-name">
        <h4 title={label || 'Untitled'}>{label || 'Untitled'}</h4>
      </div>
    </div>
  );
}

const SLOT_STEP = 28;
const TRANSITION_WIDTH = 200;
const TRANSITION_HEIGHT = 72;

export function nodeHeightForDegree(inCount, outCount, isTransition = false) {
  const base = isTransition ? TRANSITION_HEIGHT : NODE_HEIGHT;
  const slots = Math.max(1, Number(inCount) || 0, Number(outCount) || 0);
  return base + Math.max(0, slots - 1) * SLOT_STEP;
}

export function nodeWidthForKind(isTransition = false) {
  return isTransition ? TRANSITION_WIDTH : NODE_WIDTH;
}

export function buildPortItems(inCount, outCount, width, height) {
  const ins = Math.max(1, Number(inCount) || 1);
  const outs = Math.max(1, Number(outCount) || 1);
  const items = [];
  for (let i = 0; i < outs; i++) {
    const y = ((i + 1) / (outs + 1)) * height;
    items.push({
      id: `out${i}`,
      group: 'out',
      args: { x: width - 1, y },
    });
  }
  for (let i = 0; i < ins; i++) {
    const y = ((i + 1) / (ins + 1)) * height;
    items.push({
      id: `in${i}`,
      group: 'in',
      args: { x: 0, y },
    });
  }
  items.push({ id: 'outLeft', group: 'outSide', args: { x: 1, y: height / 2 } });
  items.push({ id: 'outTop', group: 'outSide', args: { x: width / 2, y: 1 } });
  items.push({
    id: 'outBottom',
    group: 'outSide',
    args: { x: width / 2, y: height - 1 },
  });
  items.push({
    id: 'inRight',
    group: 'in',
    args: { x: width - 1, y: height / 2 },
  });
  items.push({ id: 'inTop', group: 'in', args: { x: width / 2, y: 1 } });
  items.push({
    id: 'inBottom',
    group: 'in',
    args: { x: width / 2, y: height - 1 },
  });
  return items;
}

export function applyNodeSlots(node, { inCount = 1, outCount = 1, isTransition = false } = {}) {
  if (!node) return;
  const w = nodeWidthForKind(isTransition);
  const h = nodeHeightForDegree(inCount, outCount, isTransition);
  node.prop('isTransition', isTransition);
  node.resize(w, h);
  node.prop('ports/items', buildPortItems(inCount, outCount, w, h));
}

register({
  shape: "stage_node",
  width: NODE_WIDTH,
  height: NODE_HEIGHT,
  ports: {
    groups: {
      out: {
        markup: [{ tagName: 'circle', selector: 'circle' }],
        attrs: {
          circle: {
            r: 6,
            magnet: true,
            stroke: 'transparent',
            fill: 'transparent',
          },
        },
        position: { name: 'absolute' },
      },
      outSide: {
        markup: [{ tagName: 'circle', selector: 'circle' }],
        attrs: {
          circle: {
            r: 6,
            magnet: true,
            stroke: 'transparent',
            fill: 'transparent',
          },
        },
        position: { name: 'absolute' },
      },
      in: {
        markup: [{ tagName: 'circle', selector: 'circle' }],
        attrs: {
          circle: {
            r: 6,
            magnet: true,
            stroke: 'transparent',
            fill: 'transparent',
          },
        },
        position: { name: 'absolute' },
      },
    },
    items: buildPortItems(1, 1, NODE_WIDTH, NODE_HEIGHT),
  },
  effect: [
    'name',
    'stage',
    'scene',
    'isOrgasm',
    'fixedLen',
    'isStart',
    'hubReturns',
    'poseFamily',
    'isTransition',
    'displayName',
  ],
  component: StageNode,
});

export { NODE_WIDTH, NODE_HEIGHT, SLOT_STEP, TRANSITION_WIDTH, TRANSITION_HEIGHT };

export const OUT_PORT_BY_SIDE = {
  right: 'out0',
  left: 'outLeft',
  top: 'outTop',
  bottom: 'outBottom',
};
export const IN_PORT_BY_SIDE = {
  left: 'in0',
  right: 'inRight',
  top: 'inTop',
  bottom: 'inBottom',
};
