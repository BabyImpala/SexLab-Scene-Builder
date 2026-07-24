import React, { useState, useRef, useEffect } from "react";
import { emit, listen } from '@tauri-apps/api/event'
import { invoke } from "@tauri-apps/api/core"
import ReactDOM from "react-dom/client";
import { useImmer } from "use-immer";
import { FileDoneOutlined, TagsOutlined, SaveOutlined, TeamOutlined } from '@ant-design/icons';
import { Input, Button, Tooltip, InputNumber, Card, Layout, Row, Col, Tabs, notification, Collapse, ConfigProvider } from 'antd';

import { tagsSFW, tagsNSFW } from "./common/Tags"
import PositionField from "./stage/PositionField";
import TagTree from "./components/TagTree";
import "./stage.css";
import "./App.css";
// import "./Dark.css";
import { getAppTheme } from "./common/theme";
import { applyRootDarkClass, readOsDarkMode, writeStoredDarkMode } from "./common/darkMode";

const { Header, Content } = Layout;
const { TextArea } = Input;

let root = null;
document.addEventListener('DOMContentLoaded', async () => {
  const load = ({ scene, stage, positions, dark }) => {
    console.log("Scene ID:", scene, "Stage:", stage);
    const stagePositions = stage.positions || [];
    const scenePositions = positions || [];
    const n = Math.max(stagePositions.length, scenePositions.length);
    const blankInfo = () => ({
      sex: { male: true, female: false, futa: false },
      race: 'Human',
      scale: 1.0,
      submissive: false,
      vampire: false,
      dead: false,
      add_cum: 0,
    });
    const blankPos = () => ({
      event: [],
      anim_obj: '',
      offset: { x: 0, y: 0, z: 0, r: 0 },
      strip_data: {
        default: true,
        everything: false,
        nothing: false,
        helmet: false,
        gloves: false,
        boots: false,
      },
      climax: false,
      tags: [],
      schlong: 0,
      add_cum: 0,
    });
    const merged = Array.from({ length: n }, (_, i) => ({
      position: stagePositions[i] || blankPos(),
      info: scenePositions[i] || blankInfo(),
    }));
    const initialDark = typeof dark === 'boolean' ? dark : readOsDarkMode();
    writeStoredDarkMode(initialDark);
    applyRootDarkClass(initialDark);
    if (!root) root = ReactDOM.createRoot(document.getElementById("root"));
    root.render(
      <React.StrictMode>
        <Editor
          key={`Editor-${stage.id}`}
          _sceneId={scene}
          _stage={{ ...stage, positions: merged.map((m) => m.position) }}
          _positions={merged}
          _initialDark={initialDark}
        />
      </React.StrictMode>
    );
  }
  // Keep listening so a re-focused existing editor can receive a fresh payload.
  await listen('on_data_received', ({ payload }) => {
    window.sessionStorage.setItem('origin_data', JSON.stringify(payload));
    load(payload);
  });
  await emit('on_request_data');
});

function makePositionTab(p, i) {
  return { key: `PTab${i}`, position: p.position, info: p.info }
}

function Editor({ _sceneId, _stage, _positions, _initialDark }) {
  const [isDark, setIsDark] = useState(() =>
    typeof _initialDark === 'boolean' ? _initialDark : readOsDarkMode()
  );
  const [api, contextHolder] = notification.useNotification();

  const [name, setName] = useState(_stage.name);
  const [positions, updatePositions] = useImmer(_positions.map((p, i) => { return makePositionTab(p, i) }));
  const [activePosition, setActivePosition] = useState(positions[0].key);
  const positionIdx = useRef(_positions.length);
  const [tags, setTags] = useState(_stage.tags || []);
  const [fixedLen, setFixedLen] = useState(_stage.extra?.fixed_len);
  const [navText, setNavText] = useState(_stage.extra?.nav_text || '');

  useEffect(() => {
    const unlisten = listen('toggle_darkmode', (event) => {
      setIsDark(event.payload);
    });
    invoke('get_in_darkmode').then(setIsDark);
    return () => {
      unlisten.then(f => f());
    };
  }, []);

  useEffect(() => {
    writeStoredDarkMode(isDark);
    applyRootDarkClass(isDark);
  }, [isDark]);


  useEffect(() => {
    const position_remove = listen('on_position_remove', (event) => {
      const { sceneId, positionIdx } = event.payload;
      if (sceneId !== _sceneId) return;
      updatePositions(p => { p.splice(positionIdx, 1) });
    });
    const position_add = listen('on_position_add', (event) => {
      const { sceneId, position } = event.payload;
      if (sceneId !== _sceneId) return;
      updatePositions(prev => { prev.push(position) });
    });
    const position_change = listen('on_position_change', (event) => {
      const { sceneId, stageId, positionIdx, info } = event.payload;
      if (sceneId !== _sceneId || stageId === _stage.id) return;
      console.log("Position Change Event:", info);
      updatePositions(p => { p[positionIdx].info = info });
    });
    return () => {
      position_remove.then(res => { res() });
      position_add.then(res => { res() });
      position_change.then(res => { res() });
    }
  }, []);

  function saveAndReturn() {
    let positionArg = [];
    let positionsInfo = [];
    for (let i = 0; i < positions.length; i++) {
      const { position: stage_p, info: scene_p } = positions[i];
      if (!stage_p.event[0]) {
        api.error({
          message: 'Missing Event',
          description: `Position ${i + 1} is missing its behavior file (.hkx)`,
          placement: 'bottomLeft',
        });
        return;
      }
      if (!scene_p.sex.male && !scene_p.sex.female && !scene_p.sex.futa) {
        api.error({
          message: 'Missing Sex',
          description: `Position ${i + 1} has no sex assigned. Every position should be compatible with at least one sex.`,
          placement: 'bottomLeft',
        });
        return;
      }
      const animRaw = Array.isArray(stage_p.anim_obj)
        ? stage_p.anim_obj.filter(Boolean).join(' ')
        : String(stage_p.anim_obj ?? '');
      positionArg.push({
        ...stage_p,
        anim_obj: animRaw
          .split(/[,\s]+/)
          .map((s) => s.trim())
          .filter(Boolean)
          .join(','),
      });
      positionsInfo.push(scene_p);
    }
    const stage = {
      id: _stage.id,
      name,
      positions: positionArg,
      tags,
      extra: {
        fixed_len: fixedLen || 0.0,
        nav_text: navText || '',
      },
    };
    console.log("Saving Stage... ", _sceneId, positionsInfo, stage);
    invoke('stage_save_and_close', { scene: _sceneId, positions: positionsInfo, stage });
  }

  const onPositionTabEdit = (targetKey, action) => {
    if (action === 'add') {
      invoke('make_position').then((res) => {
        const next = makePositionTab(res, positionIdx.current++);
        emit('on_position_add', { sceneId: _sceneId, position: next }).then(() => {
          setActivePosition(next.key);
        });
      });
    } else {
      const id = positions.findIndex(v => v.key === targetKey);
      if (activePosition === targetKey) {
        const newidx = id > 0 ? id - 1 : 1;
        setActivePosition(positions[newidx].key);
      }
      emit('on_position_remove', { sceneId: _sceneId, positionIdx: id });
    }
  };

  const positionsCollapsed = [
    { // Tags
      key: '1',
      label: 'Tags',
      extra: <TagsOutlined />,
      children:
        <div className="tag-display-box">
          <TagTree
            tags={tags}
            onChange={setTags}
            tagsSFW={tagsSFW}
            tagsNSFW={tagsNSFW}
          />
        </div>
    },
    { // Positions
      key: '2',
      label: 'Positions',
      extra: <TeamOutlined />,
      children:
        <Tabs
          type="editable-card"
          activeKey={activePosition}
          hideAdd={positions.length > 4}
          onEdit={onPositionTabEdit}
          onChange={(e) => {
            setActivePosition(e);
          }}
          items={positions.map((p, i) => {
            return {
              label: `Position ${i + 1}`,
              closable: positions.length > 1,
              key: p.key,
              children: (
                <div className="position">
                  <PositionField
                    position={p.position}
                    info={p.info}
                    onChange={(newPosition, newInfo) => {
                      updatePositions((draft) => {
                        draft[i].position = newPosition;
                        draft[i].info = newInfo;
                      });
                      emit('on_position_change', {
                        sceneId: _sceneId,
                        stageId: _stage.id,
                        positionIdx: i,
                        info: newInfo,
                      });
                    }}
                  />
                </div>
              ),
            };
          })}
        />
    },
    { //Extra
      key: '3',
      label: 'Extra',
      extra: <FileDoneOutlined />,
      children:
        <>
          <Row gutter={[2, 2]}>
            <Col span={12}>
              <Card
                style={{ height: '100%' }}
                title={'Navigation'}
                extra={
                  <Tooltip
                    title={
                      'A short text for the player to read when given the option to branch into this stage.'
                    }
                  >
                    <Button type="text">Info</Button>
                  </Tooltip>
                }
              >
                <TextArea
                  className="extra-navinfo-textarea"
                  maxLength={100}
                  showCount
                  rows={3}
                  style={{ resize: 'none', width: '100%' }}
                  defaultValue={_stage.extra.navText}
                  value={navText}
                  onChange={(e) => setNavText(e.target.value)}
                ></TextArea>
              </Card>
            </Col>
            <Col span={12}>
              <Card
                style={{ height: '100%' }}
                title={'Fixed Duration'}
                extra={
                  <Tooltip
                    title={
                      'Duration of an animation that should only play once (does not loop).'
                    }
                  >
                    <Button type="text">Info</Button>
                  </Tooltip>
                }
              >
                <InputNumber
                  className="extra-duration-input"
                  controls
                  precision={0}
                  step={10}
                  defaultValue={_stage.extra.fixedLen}
                  min={0}
                  value={fixedLen ? fixedLen : undefined}
                  onChange={(e) => setFixedLen(e)}
                  placeholder="0"
                  addonAfter={'ms'}
                  style={{ width: '100%' }}
                />
              </Card>
            </Col>
          </Row>
        </>
    }
  ]

  return (
    <ConfigProvider theme={getAppTheme(isDark)}>
      <Layout style={{ minHeight: '100vh' }}>
        {contextHolder}
        <Header className="stage-header">
          <Row align="middle" justify="space-between" wrap={false} style={{ width: '100%' }}>
            <Col flex="none">
              <Input
                id="stage-namefield-input"
                className="stage-namefield"
                size="large"
                maxLength={30}
                bordered={false}
                value={name}
                onChange={(e) => setName(e.target.value)}
                defaultValue={_stage.name}
                placeholder={'Stage Name'}
                onFocus={(e) => e.target.select()}
              />
            </Col>
            <Col flex="none">
              <Button type="text" icon={<SaveOutlined />} onClick={saveAndReturn}>
                Save
              </Button>
            </Col>
          </Row>
        </Header>
        <Content className="stage-body">
          <Collapse items={positionsCollapsed} defaultActiveKey={['1', '2', '3']} />
        </Content>
      </Layout>
    </ConfigProvider>
  )
}

export default Editor;
