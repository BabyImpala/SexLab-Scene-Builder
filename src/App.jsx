import { useState, useEffect, useRef, useMemo } from "react";
import { useImmer } from "use-immer";
import { invoke } from "@tauri-apps/api/core";
import { listen, emit } from "@tauri-apps/api/event";
import { Graph, Shape } from '@antv/x6'
import { History } from "@antv/x6-plugin-history";
import { Menu, Layout, Card, Input, Space, Button, Empty, Modal, Tooltip, notification, Divider, Switch, Checkbox, Row, Col, InputNumber, Select, ConfigProvider, Dropdown, Segmented, Typography } from 'antd'
import {
  ExperimentOutlined, FolderOutlined, PlusOutlined, ExclamationCircleOutlined, QuestionCircleOutlined, DiffOutlined, ZoomInOutlined, ZoomOutOutlined,
  DeleteOutlined, DoubleLeftOutlined, DoubleRightOutlined, PicCenterOutlined, CompressOutlined, PushpinOutlined, DragOutlined, WarningOutlined,
  ApartmentOutlined, DownloadOutlined, UndoOutlined, UnorderedListOutlined
} from '@ant-design/icons';
import { Panel, PanelGroup, PanelResizeHandle } from 'react-resizable-panels';
import './ResizableSidebar.css';
const { Header, Content, Footer, Sider } = Layout;
const { confirm } = Modal;
import { STAGE_EDGE, STAGE_EDGE_SHAPEID, forwardEdgeAttrs } from "./scene/SceneEdge"
import { Furnitures } from "./common/Furniture";
import "./scene/SceneNode"
import { applyNodeSlots } from "./scene/SceneNode"
import { isTransitionStage, uniqueStageLabel, disambiguateDuplicateStageNames } from "./scene/stageFamily"
import {
  graphCoordsStacked,
  planToEdgeConfig,
  applyEdgePlan,
} from "./scene/graphLayout"
import {
  computeGraphPresentation,
  applyEdgeVisibility,
  resolveVisibleKeys,
  applyNodeFamilyDim,
  applyGraphLayerDim,
  sceneGraphSignature,
} from "./scene/graphPresentation"
import {
  expandCanvasToStoredGraph,
  shortTransitionLabel,
} from "./scene/transitionCollapse"
import {
  buildCanvasSvg,
  buildCanvasLayoutJson,
  defaultGraphExportName,
} from "./scene/exportCanvasSvg"
import { connectionsToCsv } from "./components/GraphConnectionsTable"
import GraphNavOutline from "./components/GraphNavOutline"
import GraphNodeSearch from "./components/GraphNodeSearch"
import { LARGE_SCENE_NODE_THRESHOLD } from "./scene/graphLayoutClusters"
import { pathToNode } from "./scene/spanningForest"
import "./App.css";
// import "./Dark.css";
import ScenePosition from "./scene/ScenePosition";
import { getAppTheme } from "./common/theme";
import { applyRootDarkClass, readOsDarkMode, writeStoredDarkMode } from "./common/darkMode";
function makeMenuItem(label, key, icon, children, disabled, danger) {
  return { key, icon, children, label, disabled, danger };
}
import { tagsSFW, tagsNSFW } from "./common/Tags"
import TagTree from "./components/TagTree";
import { remove } from "@tauri-apps/plugin-fs";
import { save } from "@tauri-apps/plugin-dialog";

const ZOOM_OPTIONS = { minScale: 0.25, maxScale: 5 };

function graphGridArgs(dark) {
  return [
    {
      thickness: 1,
      color: dark ? 'rgba(255,255,255,0.08)' : '#d0d0d4',
    },
    {
      color: dark ? 'rgba(255,255,255,0.16)' : 'rgba(33, 35, 48, 0.18)',
      thickness: dark ? 1 : 1.25,
      factor: 8,
    },
  ];
}

function App() {
  const [isDark, setIsDark] = useState(readOsDarkMode);
  const [collapsed, setCollapsed] = useState(false);  // Sider collapsed?
  const [api, contextHolder] = notification.useNotification();
  const graphcontainer_ref = useRef(null);
  const [graph, setGraph] = useState(null);
  const [scenes, updateScenes] = useImmer([]);
  const [activeScene, updateActiveScene] = useImmer(null);
  const [packName, setPackName] = useState('');
  const [packAuthor, setPackAuthor] = useState('');
  const [packVersion, setPackVersion] = useState('');
  const [edited, setEditedState] = useState(false);
  const editedRef = useRef(false);
  const setEdited = (v) => {
    const next = !!v;
    editedRef.current = next;
    setEditedState(next);
  };
  const [cloneToOpen, setCloneToOpen] = useState(false);
  const [cloneToStage, setCloneToStage] = useState(null);
  const [cloneToSourceScene, setCloneToSourceScene] = useState(null);
  const [cloneToTargetId, setCloneToTargetId] = useState(null);
  const inEdit = useRef(false);
  const [showAreas, setShowAreas] = useState(false);
  const [graphWorkMode, setGraphWorkMode] = useState('browse'); // browse | edit
  const [edgeFilterMode, setEdgeFilterMode] = useState('primary'); // primary | neighborhood | family | all
  const [focusNodeIds, setFocusNodeIds] = useState([]);
  const [mapFamilyFilter, setMapFamilyFilter] = useState('all');
  const [navOutline, setNavOutline] = useState([]);
  const [showOutline, setShowOutline] = useState(true);
  const [pathIds, setPathIds] = useState([]);
  const [transitionLayerMode, setTransitionLayerMode] = useState('collapsed');
  const transitionLayerModeRef = useRef('collapsed');
  const fullGraphRef = useRef({});
  const graphMetaRef = useRef({
    families: new Map(),
    hubReturnCounts: new Map(),
    clusters: [],
    forest: null,
  });
  const presentationCacheRef = useRef(null);
  const layoutDirtyRef = useRef(false);
  const edgeFilterModeRef = useRef(edgeFilterMode);
  const focusNodeIdsRef = useRef(focusNodeIds);
  const mapFamilyFilterRef = useRef(mapFamilyFilter);
  const graphWorkModeRef = useRef(graphWorkMode);
  /** Packed positions at scene open (before arrange) — snap-back target. */
  const layoutSnapshotRef = useRef(null);
  const refreshGraphEdgesRef = useRef(() => {});
  const rebuildGraphPresentationRef = useRef(() => {});
  const activeSceneRef = useRef(null);
  const insertingTransitionRef = useRef(false);
  const suppressingNodeRemoveRef = useRef(false);

  useEffect(() => {
    activeSceneRef.current = activeScene;
  }, [activeScene]);

  useEffect(() => {
    transitionLayerModeRef.current = transitionLayerMode;
  }, [transitionLayerMode]);

  useEffect(() => {
    edgeFilterModeRef.current = edgeFilterMode;
  }, [edgeFilterMode]);

  useEffect(() => {
    focusNodeIdsRef.current = focusNodeIds;
  }, [focusNodeIds]);

  useEffect(() => {
    mapFamilyFilterRef.current = mapFamilyFilter;
  }, [mapFamilyFilter]);

  useEffect(() => {
    graphWorkModeRef.current = graphWorkMode;
  }, [graphWorkMode]);

  const familyFilterOptions = useMemo(() => {
    const fam = graphMetaRef.current.families;
    if (!fam?.size) return [];
    return [...new Set(fam.values())].sort();
  }, [navOutline, activeScene?.id]);

  function generatePositionId() {
    return `${Date.now()}_${Math.random().toString(36).substr(2, 9)}`;
  }

  useEffect(() => {
    const unlisten = listen('toggle_darkmode', (event) => {
      setIsDark(event.payload);
    });
    invoke('get_in_darkmode').then(setIsDark).catch(() => {});
    return () => {
      unlisten.then(f => f());
    };
  }, []);

  useEffect(() => {
    writeStoredDarkMode(isDark);
    applyRootDarkClass(isDark);
  }, [isDark]);

  // Graph
  useEffect(() => {
    const newGraph = new Graph({
      container: graphcontainer_ref.current,
      grid: {
        visible: true,
        size: 20,
        type: 'doubleMesh',
        args: graphGridArgs(isDark),
      },
      panning: true,
      autoResize: true,
      mousewheel: {
        enabled: true,
        minScale: ZOOM_OPTIONS.minScale,
        maxScale: ZOOM_OPTIONS.maxScale,
        // modifiers: ['ctrl']
      },
      connecting: {
        allowBlank: false,
        allowMulti: false,
        allowLoop: false,
        allowEdge: false,
        allowPort: true,
        allowNode: true,
        createEdge() {
          return new Shape.Edge({
            shape: STAGE_EDGE_SHAPEID,
            ...STAGE_EDGE,
            attrs: forwardEdgeAttrs(isDark),
          });
        },
      }
    })
      .zoomTo(1.0)
      .use(new History({
        enabled: true,
      }));

    newGraph // Node Events
      .on("node:removed", ({ node }) => {
        if (inEdit.current || suppressingNodeRemoveRef.current) return;
        updateActiveScene(prev => {
          if (prev.root === node.id) {
            prev.root = null;
          }
          prev.stages = prev.stages.filter(it => it.id !== node.id);
        })
        setEdited(true);
      })
      .on("node:added", (evt) => {
        if (inEdit.current) return;
        setEdited(true);
      })
      .on("node:moved", ({ e, x, y, node, view }) => {
        const box = node.getBBox();
        const views = newGraph.findViewsInArea(box);
        views.forEach(it => {
          if (!it.isEdgeView()) {
            return;
          }
          it.update();
        });
        layoutDirtyRef.current = true;
        if (inEdit.current) return;
        setEdited(true);
      })
      .on("node:mouseup", () => {
        if (!layoutDirtyRef.current) return;
        layoutDirtyRef.current = false;
        presentationCacheRef.current = null;
        queueMicrotask(() => rebuildGraphPresentationRef.current?.());
      })
      .on("edge:dblclick", ({ edge }) => {
        const via = edge.getData?.()?.viaStageId;
        if (!via) return;
        const scene = activeSceneRef.current;
        const stage = scene?.stages?.find((s) => s.id === via);
        if (!stage || !scene) return;
        invoke('open_stage_editor', {
          sceneId: scene.id,
          positions: scene.positions || [],
          stage,
          existingStageCount: scene.stages?.length || 0,
          templateStage: null,
        });
      })
      .on("edge:contextmenu", ({ e, edge }) => {
        e.stopPropagation();
        const via = edge.getData?.()?.viaStageId;
        const viaName = edge.getData?.()?.viaName;
        confirm({
          title: via ? 'Remove transition connection?' : 'Remove connection?',
          content: via
            ? `This link goes through "${viaName || via}". Remove the connection?`
            : 'Delete this edge between poses?',
          okText: 'Remove',
          cancelText: 'Cancel',
          onOk() {
            edge.remove();
            setEdited(true);
            if (via && activeSceneRef.current) {
              confirm({
                title: 'Remove transition stage?',
                content: `Also delete transition "${viaName || via}" from the scene?`,
                okText: 'Delete transition',
                cancelText: 'Keep stage',
                onOk() {
                  updateActiveScene((prev) => {
                    prev.stages = (prev.stages || []).filter((s) => s.id !== via);
                  });
                  const g = fullGraphRef.current || {};
                  delete g[via];
                  for (const id of Object.keys(g)) {
                    g[id].dest = (g[id].dest || []).filter((d) => d !== via);
                  }
                  fullGraphRef.current = g;
                  presentationCacheRef.current = null;
                  queueMicrotask(() => rebuildGraphPresentationRef.current?.());
                },
                onCancel() {
                  presentationCacheRef.current = null;
                  queueMicrotask(() => rebuildGraphPresentationRef.current?.());
                },
              });
            } else {
              presentationCacheRef.current = null;
              queueMicrotask(() => rebuildGraphPresentationRef.current?.());
            }
          },
        });
      })
      .on("edge:connected", ({ edge }) => {
        if (inEdit.current || insertingTransitionRef.current) return;
        setEdited(true);
        if (transitionLayerModeRef.current !== 'collapsed') {
          presentationCacheRef.current = null;
          queueMicrotask(() => rebuildGraphPresentationRef.current?.());
          return;
        }
        const source = edge.getSourceCellId();
        const target = edge.getTargetCellId();
        confirm({
          title: 'Insert transition stage?',
          content:
            'Create a fixed-length transition between these poses (OStim-style), or keep a direct link?',
          okText: 'Insert transition',
          cancelText: 'Direct link',
          onOk() {
            insertingTransitionRef.current = true;
            const scene = activeSceneRef.current;
            if (!scene || !source || !target) {
              insertingTransitionRef.current = false;
              return;
            }
            const id = Math.random().toString(36).slice(2, 10);
            const tgtName =
              scene.stages?.find((s) => s.id === target)?.name || 'Pose';
            const short = shortTransitionLabel(tgtName);
            const newStage = {
              id,
              name: `Go to ${short}`,
              positions: JSON.parse(
                JSON.stringify(
                  scene.stages?.find((s) => s.id === source)?.positions || []
                )
              ),
              tags: ['transition'],
              extra: { fixed_len: 1, nav_text: '', sound: '' },
            };
            updateActiveScene((prev) => {
              prev.stages = [...(prev.stages || []), newStage];
            });
            edge.setData({ viaStageId: id, viaName: newStage.name });
            edge.setLabels([
              {
                attrs: {
                  label: { text: shortTransitionLabel(newStage.name), fontSize: 11 },
                },
              },
            ]);
            insertingTransitionRef.current = false;
            presentationCacheRef.current = null;
            queueMicrotask(() => rebuildGraphPresentationRef.current?.());
          },
          onCancel() {
            presentationCacheRef.current = null;
            queueMicrotask(() => rebuildGraphPresentationRef.current?.());
          },
        });
      })
      // Custom Events
      .on("node:doMarkRoot", ({ node }) => {
        updateActiveScene(prev => {
          const cell = newGraph.getCellById(prev.root);
          if (cell) { cell.prop('isStart', false); }
          node.prop('isStart', true);
          prev.root = node.id;
        });
        setEdited(true);
      })
      .on("node:clone", ({ node }) => {
        // Prefer live scene/stage data — node props go stale when actors are
        // added/removed from another stage editor in the same animation.
        const live = activeSceneRef.current;
        const belonging = node.prop('scene');
        const scene =
          live && belonging && live.id === belonging.id ? live : belonging;
        const stage =
          scene?.stages?.find((s) => s.id === node.id) || node.prop('stage');
        invoke('open_stage_editor_from', {
          sceneId: scene.id,
          positions: scene.positions || [],
          copyStage: stage,
          existingStageCount: scene.stages?.length || 0,
        });
      })
      .on("node:cloneTo", ({ node }) => {
        const live = activeSceneRef.current;
        const belonging = node.prop('scene');
        const sourceScene =
          live && belonging && live.id === belonging.id ? live : belonging;
        const stage =
          sourceScene?.stages?.find((s) => s.id === node.id) ||
          node.prop('stage');
        setCloneToStage(stage);
        setCloneToSourceScene(sourceScene);
        setCloneToTargetId(null);
        setCloneToOpen(true);
      })
      .on('node:click', ({ node }) => {
        setFocusNodeIds([node.id]);
        focusNodeIdsRef.current = [node.id];
        const forest = graphMetaRef.current.forest;
        if (forest?.parent) {
          setPathIds(pathToNode(node.id, forest.parent));
        }
        queueMicrotask(() => refreshGraphEdgesRef.current?.());
      })

    setGraph(newGraph);
    return () => {
      newGraph.dispose();
      if (graphcontainer_ref.current) {
        graphcontainer_ref.current.innerHTML = '';
      }
    }
  }, []);

  useEffect(() => {
    if (!graph) return;
    graph.drawGrid({
      type: 'doubleMesh',
      args: graphGridArgs(isDark),
    });
  }, [graph, isDark]);

  useEffect(() => {
    if (!graph) return;

    const editStage = (node) => {
      // Live stage from activeScene — node.prop('stage') lags when actors are
      // added via another stage in this animation.
      let stage =
        activeScene?.stages?.find((s) => s.id === node.id) ||
        node.prop('stage');
      console.log("Editing stage", stage, "in scene", activeScene);

      console.assert(activeScene.stages.findIndex(it => it.id === stage.id) > -1, "Editing stage that does not belong to active scene: ", stage, activeScene);
      invoke('open_stage_editor', {
        sceneId: activeScene.id,
        positions: activeScene.positions || [],
        stage,
        existingStageCount: activeScene.stages?.length || 0,
        templateStage: null,
      });
    }

    graph
      .on('node:dblclick', ({ node }) => {
        editStage(node);
      })
      .on("node:edit", ({ node }) => {
        editStage(node);
      })
    return () => {
      graph.off('node:dblclick');
      graph.off('node:edit');
    }
  }, [graph, activeScene])

  // Stage & Scene update
  useEffect(() => {
    // Callback after stage has been saved in other window
    const stage_save = listen('on_stage_saved', (event) => {
      const { scene, positions, stage } = event.payload;
      console.log("Saving new stage in ", scene, positions, stage);
      const sceneId = typeof scene === 'string' ? scene : scene?.id ?? scene;
      const updatingActiveScene =
        scenes.length === 0 || activeScene?.id === sceneId;
      let updatedScene = undefined;
      let updatedSceneIdx = -1;
      let node = undefined;
      if (updatingActiveScene) {
        const nodes = graph.getNodes();
        node = nodes.find((n) => n.id === stage.id);
        if (!node) node = addStageToGraph(stage);
        updateNodeProps(stage, node, activeScene);
        updatedScene = activeScene;
      } else {
        updatedSceneIdx = scenes.findIndex((it) => it.id === sceneId);
        if (updatedSceneIdx === -1) {
          // Destination missing from the sidebar list (e.g. created but never
          // flushed). Still accept the clone using the editor payload.
          console.warn(
            'Scene not in list; creating from clone payload',
            sceneId,
            scenes
          );
          updatedScene = {
            id: sceneId,
            name: '',
            stages: [],
            root: stage.id,
            graph: {},
            furniture: {
              enabled: false,
              id: '',
              offset: { x: 0, y: 0, z: 0, r: 0 },
            },
            private: false,
            tags: [],
            positions: [],
            has_warnings: false,
          };
        } else {
          updatedScene = scenes[updatedSceneIdx];
        }
      }
      updatedScene = structuredClone(updatedScene);
      let editedStageIdx =
        updatedScene.stages?.findIndex((it) => it.id === stage.id) ?? -1;
      if (editedStageIdx === -1) {
        updatedScene.stages = updatedScene.stages || [];
        updatedScene.stages.push(stage);
        if (updatedScene.stages.length === 1) {
          if (node) node.prop('isStart', true);
          updatedScene.root = stage.id;
        }
        // Always ensure graph placement for non-active destinations (and for
        // active ones the X6 node already exists).
        if (!updatingActiveScene) {
          const g = { ...(updatedScene.graph || {}) };
          if (!g[stage.id]) {
            const count = Object.keys(g).length;
            g[stage.id] = {
              dest: [],
              x: 40 + (count % 4) * 220,
              y: 40 + Math.floor(count / 4) * 140,
            };
          }
          updatedScene.graph = g;
          if (!updatedScene.root) {
            updatedScene.root = stage.id;
          }
        }
      } else {
        updatedScene.stages[editedStageIdx] = stage;
      }
      updatedScene.positions = positions;
      if (updatingActiveScene) {
        updateActiveScene(updatedScene);
        setEdited(true);
      } else {
        invoke('save_scene', { scene: updatedScene })
          .then(() => {
            updateScenes((prev) => {
              const idx = prev.findIndex((s) => s.id === updatedScene.id);
              if (idx === -1) prev.push(updatedScene);
              else prev[idx] = updatedScene;
            });
            // Do not setEdited(true): the active (source) animation was not
            // modified. save_scene already marks the project dirty in Rust.
            api.success({
              message: 'Stage cloned',
              description: `Added to “${updatedScene.name || 'Untitled'}”. Open that animation to see it.`,
              placement: 'bottomLeft',
            });
          })
          .catch((err) => {
            console.error(err);
            api.error({
              message: 'Failed to save cloned stage',
              description: String(err),
              placement: 'bottomLeft',
            });
          });
      }
    });
    const position_remove = listen('on_position_remove', (event) => {
      const { sceneId, positionIdx } = event.payload;
      console.log("Removing position", positionIdx, "from scene", sceneId);

      const remove_position = (scene) => {
        // Remove from each stage
        scene.stages.forEach(stage => {
          if (positionIdx >= 0 && positionIdx < stage.positions.length) {
            stage.positions = stage.positions.filter((_, idx) => idx !== positionIdx);
          }
        });
        // Remove from scene.positions
        scene.positions = scene.positions.filter((_, idx) => idx !== positionIdx);
        scene.has_warnings = true;
      };
      if (scenes.length === 0 || activeScene.id === sceneId) {
        updateActiveScene(draft => remove_position(draft));
      } else {
        updateScenes(draft => {
          const idx = draft.findIndex(it => it.id === sceneId);
          if (idx === -1) return;
          remove_position(draft[idx]);
        });
      }
    });
    const position_add = listen('on_position_add', (event) => {
      const { sceneId, position } = event.payload;
      console.log("Adding position", position, "to scene", sceneId);

      const add_position = (scene) => {
        // Always clone and assign a unique id
        const newPosition = { ...position.info, id: generatePositionId() };
        scene.stages.forEach(stage => {
          stage.positions.push({ ...position.position, id: generatePositionId() });
        });
        scene.positions.push(newPosition);
        scene.has_warnings = true;
      };

      if (scenes.length === 0 || activeScene.id === sceneId) {
        updateActiveScene(draft => add_position(draft));
      } else {
        updateScenes(draft => {
          const idx = draft.findIndex(it => it.id === sceneId);
          if (idx === -1) return;
          add_position(draft[idx]);
        });
      }
    });
    const position_change = listen('on_position_change', (event) => {
      const { sceneId, stageId, positionIdx, info } = event.payload;
      if (stageId === 0) return // invoked by ScenePosition, skip
      // Skip position change if the scene is not currently active
      // If the stage of an inactive scene is saved, the info will be updated accordingly
      if (scenes.length === 0 || activeScene.id === sceneId) {
        updateActiveScene(draft => {
          // Always clone and assign a unique id
          const newPosition = { ...info, id: generatePositionId() };
          draft.positions[positionIdx] = newPosition;
        });
      }
    });
    return () => {
      console.log("Active before update:", activeScene);
      stage_save.then(res => { res() });
      position_remove.then(res => { res() });
      position_add.then(res => { res() });
      position_change.then(res => { res() });
    }
  }, [graph, activeScene, scenes])

  useEffect(() => {
    if (!graph) return;
    const unlisten = listen('on_project_update', (event) => {
      const payload = event.payload || {};
      const stage_map = payload.scenes ?? payload;
      const scns = [];
      for (const key in stage_map) {
        if (Object.hasOwnProperty.call(stage_map, key)) {
          const element = stage_map[key];
          scns.push(element);
        }
      }
      console.log("Opening new Project with Scenes: ", scns);
      for (const scene of scns) {
        disambiguateDuplicateStageNames(scene.stages || []);
      }
      updateScenes(scns);
      setPackName(payload.pack_name ?? '');
      setPackAuthor(payload.pack_author ?? '');
      setPackVersion(payload.pack_version ?? '');
      setEdited(false);
      if (scns.length) {
        // Show side panels before loading the scene so graph fit uses the
        // final layout width (same as Edit from the sidebar).
        setShowAreas(true);
        setActiveScene(scns[0]);
      } else {
        updateActiveScene(null);
        setShowAreas(false);
      }
    });
    invoke('request_project_update');
    return () => {
      unlisten.then(res => { res() });
    }
  }, [graph])

  const clearGraph = () => {
    if (graph.getCellCount() == 0)
      return;

    confirm({
      title: 'Clear Graph',
      icon: <QuestionCircleOutlined />,
      content: 'This will remove all nodes and edges from the current scene. Do you want to continue?',
      onOk() {
        graph.clearCells();
        setEdited(true);
      }
    })
  }

  const setActiveScene = async (newscene) => {
    if (!inEdit.current && editedRef.current) {
      confirm({
        title: 'Unsaved changes',
        icon: <ExclamationCircleOutlined />,
        content: `Are you sure you want to continue? Unsaved changes will be lost.`,
        okText: 'Continue without saving',
        onOk() {
          inEdit.current = true;
          setActiveScene(newscene);
        },
        onCancel() { },
      });
      return;
    }
    inEdit.current = true;
    graph.clearCells();

    // OStim packs often reuse the same transition title for different clips.
    if (disambiguateDuplicateStageNames(newscene.stages || [])) {
      setEdited(true);
    }
    updateActiveScene(newscene);

    const sceneGraph = newscene.graph || {};
    const graphIds = Object.keys(sceneGraph);
    const getName = (id) =>
      newscene.stages?.find((s) => s.id === id)?.name || id;
    const large = graphIds.length >= LARGE_SCENE_NODE_THRESHOLD;

    // Snapshot packed coords for "Restore positions" (SLSB layout only — not OStim JSON).
    layoutSnapshotRef.current = new Map(
      graphIds.map((id) => {
        const g = sceneGraph[id] || {};
        return [id, { x: Number(g.x) || 40, y: Number(g.y) || 40 }];
      })
    );

    if (large) {
      setGraphWorkMode('browse');
      graphWorkModeRef.current = 'browse';
      setEdgeFilterMode('primary');
      edgeFilterModeRef.current = 'primary';
      setShowOutline(true);
    } else {
      setGraphWorkMode('browse');
      graphWorkModeRef.current = 'browse';
      setEdgeFilterMode('primary');
      edgeFilterModeRef.current = 'primary';
    }
    setFocusNodeIds([]);
    focusNodeIdsRef.current = [];
    setPathIds([]);
    setMapFamilyFilter('all');
    presentationCacheRef.current = null;

    const stacked = graphCoordsStacked(sceneGraph);
    // Browse defaults to forest arrange when coords are stacked or scene is large.
    const shouldArrange = stacked || large;
    const collapseTransitions = transitionLayerModeRef.current === 'collapsed';
    const presentation = computeGraphPresentation({
      sceneGraph,
      rootId: newscene.root,
      nodeIds: graphIds,
      getName,
      isDark,
      edgeMode: 'primary',
      focusNodeIds: [],
      preferCluster: false,
      rearrange: shouldArrange,
      useForestLayout: true,
      stages: newscene.stages || [],
      buildRows: false,
      collapseTransitions,
      existingPositions: shouldArrange
        ? null
        : new Map(
            graphIds.map((id) => {
              const g = sceneGraph[id] || {};
              return [id, { x: Number(g.x) || 40, y: Number(g.y) || 40 }];
            })
          ),
    });

    fullGraphRef.current = structuredClone(sceneGraph);

    graphMetaRef.current = {
      families: presentation.families,
      hubReturnCounts: presentation.hubReturnCounts,
      clusters: presentation.clusters,
      forest: presentation.forest,
    };
    presentationCacheRef.current = {
      signature: presentation.signature,
      forest: presentation.forest,
      allEdges: presentation.allEdges,
      ranks: presentation.ranks,
      families: presentation.families,
      positions: presentation.positions,
    };
    setNavOutline(presentation.outline || []);

    const visibleIds = presentation.visibleIds || graphIds;
    for (const key of visibleIds) {
      const stage = newscene.stages.find((s) => s.id === key);
      if (!stage) {
        console.warn('Graph references missing stage', key, newscene);
        continue;
      }
      const pos = presentation.positions?.get(key) || sceneGraph[key] || { x: 40, y: 40 };
      const node = addStageToGraph(stage, pos.x, pos.y);
      updateNodeProps(stage, node, newscene);
      const size = presentation.nodeSizes?.get(key);
      applyNodeSlots(node, {
        inCount: presentation.inCount?.get(key) || 1,
        outCount: presentation.outCount?.get(key) || 1,
        isTransition: !!size?.isTransition,
      });
      node.prop('poseFamily', presentation.families?.get(key) || '');
      node.prop(
        'hubReturns',
        presentation.hubReturnCounts?.get(key) || 0
      );
    }
    const nodes = graph.getNodes();
    const planByPair = new Map(
      (presentation.allEdges || presentation.edges).map((p) => [
        `${p.source}\0${p.target}`,
        p,
      ])
    );
    for (const plan of presentation.allEdges || []) {
      if (!nodes.find((node) => node.id === plan.source)) continue;
      if (!nodes.find((node) => node.id === plan.target)) continue;
      graph.addEdge(planToEdgeConfig(plan));
    }
    applyGraphLayerDim(graph, transitionLayerModeRef.current);
    applyEdgeVisibility(graph, presentation.visibleKeys);
    applyNodeFamilyDim(
      graph,
      presentation.families,
      mapFamilyFilterRef.current
    );
    setEdited(false);
    // Wait until the graph container is laid out. On first project/SLAL load the
    // scene box was display:none and/or the tags panel is still mounting, so an
    // immediate zoomToFit leaves nodes uncentered; Edit later works because the
    // container already has a size.
    const fitWhenReady = (retries = 30) => {
      requestAnimationFrame(() => {
        try {
          graph.resize();
        } catch (_) { /* container may be mid-layout */ }
        const el = graph.container;
        const ready = el && el.clientWidth > 0 && el.clientHeight > 0;
        if (!ready && retries > 0) {
          fitWhenReady(retries - 1);
          return;
        }
        if (nodes.length) {
          graph.zoomToFit({ padding: 32, maxScale: 1, minScale: 0.45 });
          graph.centerContent();
          graph.getEdges().forEach((edge) => {
            const edgeView = graph.findViewByCell(edge);
            if (edgeView) edgeView.update();
          });
        }
        inEdit.current = false;
        setEdited(false);
      });
    };
    fitWhenReady();
  }

  const gridSize = 260;

  const syncStoredGraphFromCanvas = () => {
    if (!graph || !activeSceneRef.current) return fullGraphRef.current || {};
    const nodes = graph.getNodes().map((n) => {
      const p = n.getPosition();
      return { id: n.id, x: p.x, y: p.y };
    });
    const edges = graph.getEdges().map((e) => ({
      source: e.getSourceCellId(),
      target: e.getTargetCellId(),
      viaStageId:
        e.getData?.()?.viaStageId ||
        e.prop('data')?.viaStageId ||
        null,
    }));
    const next = expandCanvasToStoredGraph({
      stages: activeSceneRef.current.stages || [],
      prevGraph: fullGraphRef.current || activeSceneRef.current.graph || {},
      nodes,
      edges,
    });
    fullGraphRef.current = next;
    return next;
  };

  const buildLiveSceneGraph = () => syncStoredGraphFromCanvas();

  const refreshGraphEdgeVisibility = () => {
    if (!graph || !activeScene) return;
    const sceneGraph = fullGraphRef.current || syncStoredGraphFromCanvas();
    const graphIds = Object.keys(sceneGraph);
    const sig = sceneGraphSignature(sceneGraph);
    const cache = presentationCacheRef.current;

    if (!cache || cache.signature !== sig) {
      rebuildGraphPresentation({ rearrange: false });
      return;
    }

    const { visibleKeys, families } = resolveVisibleKeys({
      sceneGraph: cache.viewGraph || sceneGraph,
      nodeIds: cache.visibleIds || graphIds,
      edgeMode: edgeFilterModeRef.current,
      focusNodeIds: focusNodeIdsRef.current,
      familyFilter: mapFamilyFilterRef.current,
      forest: cache.forest,
      ranks: cache.ranks,
    });

    applyGraphLayerDim(graph, transitionLayerModeRef.current);
    applyEdgeVisibility(graph, visibleKeys);
    applyNodeFamilyDim(graph, families || cache.families, mapFamilyFilterRef.current);

    const focus = focusNodeIdsRef.current?.[0];
    if (focus && cache.forest?.parent) {
      setPathIds(pathToNode(focus, cache.forest.parent));
    }
  };

  /**
   * Full path: re-rank, re-route, apply changed edge plans, refresh cache.
   * Call on topology/position changes and Arrange — not on every click.
   */
  const rebuildGraphPresentation = ({
    rearrange = false,
    rootId = null,
  } = {}) => {
    if (!graph || !activeScene) return;
    const sceneGraph = syncStoredGraphFromCanvas();
    const graphIds = Object.keys(sceneGraph);
    if (!graphIds.length) return;
    const getName = (id) =>
      activeScene.stages?.find((s) => s.id === id)?.name || id;
    const browse = graphWorkModeRef.current === 'browse';
    const startId =
      rootId ||
      activeScene.root ||
      graphIds[0];
    const presentation = computeGraphPresentation({
      sceneGraph,
      rootId: startId,
      nodeIds: graphIds,
      getName,
      isDark,
      edgeMode:
        edgeFilterModeRef.current ||
        (browse ? 'primary' : 'all'),
      focusNodeIds: focusNodeIdsRef.current,
      preferCluster: false,
      rearrange,
      useForestLayout: true,
      stages: activeScene.stages || [],
      buildRows: false,
      collapseTransitions: transitionLayerModeRef.current === 'collapsed',
      existingPositions: rearrange
        ? null
        : new Map(
            graph.getNodes().map((n) => {
              const p = n.getPosition();
              return [n.id, { x: p.x, y: p.y }];
            })
          ),
    });

    const wantIds = new Set(presentation.visibleIds || []);
    const hadIds = new Set(graph.getNodes().map((n) => n.id));
    const batch = typeof graph.startBatch === 'function';
    if (batch) graph.startBatch('rebuild-presentation');
    try {
      suppressingNodeRemoveRef.current = true;
      graph.getNodes().forEach((n) => {
        if (!wantIds.has(n.id)) n.remove();
      });
      suppressingNodeRemoveRef.current = false;
      for (const id of wantIds) {
        const stage = activeScene.stages?.find((s) => s.id === id);
        if (!stage) continue;
        let node = graph.getCellById(id);
        const pos = presentation.positions?.get(id) || {
          x: sceneGraph[id]?.x || 40,
          y: sceneGraph[id]?.y || 40,
        };
        const newlyRevealed = !hadIds.has(id);
        const isT = !!presentation.nodeSizes?.get(id)?.isTransition;
        const curPos = node?.getPosition?.();
        const parkedAtOrigin =
          !!curPos &&
          ((Math.abs(curPos.x - 40) < 1 && Math.abs(curPos.y - 40) < 1) ||
            (Math.abs(curPos.x) < 1 && Math.abs(curPos.y) < 1));
        if (!node) {
          node = addStageToGraph(stage, pos.x, pos.y);
        } else if (rearrange || newlyRevealed || (isT && parkedAtOrigin)) {
          // Newly revealed / origin-parked transition stages must leave the default corner.
          node.setPosition(pos.x, pos.y);
        }
        updateNodeProps(stage, node, activeScene);
        const size = presentation.nodeSizes?.get(id);
        applyNodeSlots(node, {
          inCount: presentation.inCount?.get(id) || 1,
          outCount: presentation.outCount?.get(id) || 1,
          isTransition: !!size?.isTransition,
        });
        node.prop('poseFamily', presentation.families?.get(id) || '');
        node.prop(
          'hubReturns',
          presentation.hubReturnCounts?.get(id) || 0
        );
        if (newlyRevealed) node.setProp('layerDim', false, { silent: true });
      }

      const planByPair = new Map(
        (presentation.allEdges || []).map((p) => [`${p.source}\0${p.target}`, p])
      );
      const wantedEdgeKeys = new Set(planByPair.keys());
      graph.getEdges().forEach((edge) => {
        const s = edge.getSourceCellId();
        const t = edge.getTargetCellId();
        const key = `${s}\0${t}`;
        const plan = planByPair.get(key);
        if (!plan) {
          edge.remove();
          return;
        }
        applyEdgePlan(edge, plan);
        wantedEdgeKeys.delete(key);
      });
      for (const key of wantedEdgeKeys) {
        const plan = planByPair.get(key);
        if (plan) graph.addEdge(planToEdgeConfig(plan));
      }
    } finally {
      if (batch) graph.stopBatch('rebuild-presentation');
    }

    graphMetaRef.current = {
      families: presentation.families,
      hubReturnCounts: presentation.hubReturnCounts,
      clusters: presentation.clusters,
      forest: presentation.forest,
    };
    presentationCacheRef.current = {
      signature: presentation.signature,
      forest: presentation.forest,
      allEdges: presentation.allEdges,
      ranks: presentation.ranks,
      families: presentation.families,
      positions: presentation.positions,
      viewGraph: presentation.collapse?.poseGraph,
      visibleIds: presentation.visibleIds,
    };
    setNavOutline(presentation.outline || []);

    applyGraphLayerDim(graph, transitionLayerModeRef.current);
    applyEdgeVisibility(graph, presentation.visibleKeys);
    applyNodeFamilyDim(
      graph,
      presentation.families,
      mapFamilyFilterRef.current
    );

    const focus = focusNodeIdsRef.current?.[0];
    if (focus && presentation.forest?.parent) {
      setPathIds(pathToNode(focus, presentation.forest.parent));
    }
  };

  refreshGraphEdgesRef.current = refreshGraphEdgeVisibility;
  rebuildGraphPresentationRef.current = () =>
    rebuildGraphPresentation({ rearrange: false });

  const arrangeStages = (rootId = activeScene?.root, markEdited = true) => {
    if (!graph?.getNodes()?.length) return;
    presentationCacheRef.current = null;
    rebuildGraphPresentation({ rearrange: true, rootId });
    try {
      graph.zoomToFit({ padding: 24, maxScale: 1 });
      graph.centerContent();
    } catch (_) { /* ignore */ }
    if (markEdited) setEdited(true);
  };

  /**
   * Restore node positions from the snapshot taken when the scene was opened
   * (packed SLSB coords). Does not change graph edges. Layout is never part of
   * OStim scene JSON — only SLSB Node {x,y}.
   */
  const restorePackedPositions = (markEdited = true) => {
    if (!graph || !layoutSnapshotRef.current) return;
    const snap = layoutSnapshotRef.current;
    for (const [id, pos] of snap) {
      const node = graph.getCellById(id);
      if (node) node.setPosition(pos.x, pos.y);
    }
    presentationCacheRef.current = null;
    queueMicrotask(() => {
      rebuildGraphPresentationRef.current?.();
      try {
        graph.zoomToFit({ padding: 24, maxScale: 1 });
        graph.centerContent();
      } catch (_) { /* ignore */ }
    });
    if (markEdited) setEdited(true);
  };

  const jumpToNode = (nodeId) => {
    if (!nodeId || !graph) return;
    setFocusNodeIds([nodeId]);
    focusNodeIdsRef.current = [nodeId];
    const forest = graphMetaRef.current.forest;
    if (forest?.parent) {
      setPathIds(pathToNode(nodeId, forest.parent));
    }
    if (graphWorkModeRef.current === 'browse') {
      setEdgeFilterMode('primary');
      edgeFilterModeRef.current = 'primary';
    } else {
      setEdgeFilterMode('neighborhood');
      edgeFilterModeRef.current = 'neighborhood';
    }
    queueMicrotask(() => {
      refreshGraphEdgesRef.current?.();
      requestAnimationFrame(() => {
        try {
          graph.resize();
          const cell = graph.getCellById(nodeId);
          if (cell) {
            graph.centerCell(cell);
            graph.zoomTo(0.9, { maxScale: 1.2, minScale: 0.4 });
            graph.select(cell);
          }
        } catch (_) { /* ignore */ }
      });
    });
  };

  const applyWorkMode = (mode) => {
    setGraphWorkMode(mode);
    graphWorkModeRef.current = mode;
    if (mode === 'browse') {
      setEdgeFilterMode('primary');
      edgeFilterModeRef.current = 'primary';
      setShowOutline(true);
    } else {
      setEdgeFilterMode(
        focusNodeIdsRef.current?.length ? 'neighborhood' : 'all'
      );
      edgeFilterModeRef.current = focusNodeIdsRef.current?.length
        ? 'neighborhood'
        : 'all';
    }
    queueMicrotask(() => refreshGraphEdgesRef.current?.());
  };

  const exportGraphCanvas = async (format = 'svg') => {
    if (!graph || graph.getCellCount() === 0) {
      api.warning({
        message: 'Nothing to export',
        description: 'Open a scene with stages on the canvas first.',
        placement: 'topRight',
      });
      return;
    }
    const sceneName = activeScene?.name || 'scene';
    const isSvg = format === 'svg';
    const defaultName = defaultGraphExportName(sceneName, isSvg ? 'svg' : 'json');

    let path;
    try {
      path = await save({
        title: isSvg ? 'Export graph SVG' : 'Export graph layout JSON',
        defaultPath: defaultName,
        filters: [
          {
            name: isSvg ? 'SVG graph' : 'Graph layout JSON',
            extensions: isSvg ? ['svg'] : ['json'],
          },
        ],
      });
    } catch (err) {
      api.error({
        message: 'Graph export failed',
        description: String(err),
        placement: 'topRight',
      });
      return;
    }
    if (!path) return;

    await new Promise((r) => setTimeout(r, 0));

    let contents;
    try {
      // Export only currently visible edges (what you see on the Map).
      contents = isSvg
        ? buildCanvasSvg(graph, {
            sceneName,
            isDark,
            onlyVisible: true,
          })
        : buildCanvasLayoutJson(graph, {
            sceneName,
            isDark,
            onlyVisible: true,
          });
    } catch (err) {
      api.error({
        message: 'Graph export failed',
        description: String(err),
        placement: 'topRight',
      });
      return;
    }

    try {
      await invoke('write_export_file', { path, contents });
      api.success({
        message: 'Graph exported',
        description: path,
        placement: 'topRight',
      });
    } catch (err) {
      api.error({
        message: 'Graph export failed',
        description: String(err),
        placement: 'topRight',
      });
    }
  };

  const exportConnectionsTable = async () => {
    if (!graph || !activeScene) {
      api.warning({
        message: 'Nothing to export',
        description: 'Open a scene first.',
        placement: 'topRight',
      });
      return;
    }
    const sceneGraph = buildLiveSceneGraph();
    const getName = (id) =>
      activeScene.stages?.find((s) => s.id === id)?.name || id;
    const presentation = computeGraphPresentation({
      sceneGraph,
      rootId: activeScene.root,
      nodeIds: Object.keys(sceneGraph),
      getName,
      isDark,
      edgeMode: 'all',
      rearrange: false,
      useForestLayout: false,
      stages: activeScene.stages || [],
      buildRows: true,
      existingPositions: new Map(
        Object.entries(sceneGraph).map(([id, n]) => [id, { x: n.x, y: n.y }])
      ),
    });
    const rows = presentation.connectionRows || [];
    const sceneName = activeScene?.name || 'scene';
    const defaultName = defaultGraphExportName(sceneName, 'csv').replace(
      '_graph.csv',
      '_connections.csv'
    );
    let path;
    try {
      path = await save({
        title: 'Export connections CSV',
        defaultPath: defaultName,
        filters: [{ name: 'CSV', extensions: ['csv'] }],
      });
    } catch (err) {
      api.error({
        message: 'Export failed',
        description: String(err),
        placement: 'topRight',
      });
      return;
    }
    if (!path) return;
    try {
      await invoke('write_export_file', {
        path,
        contents: connectionsToCsv(rows),
      });
      api.success({
        message: 'Connections exported',
        description: path,
        placement: 'topRight',
      });
    } catch (err) {
      api.error({
        message: 'Export failed',
        description: String(err),
        placement: 'topRight',
      });
    }
  };

  // Place new stages at x/y when provided; otherwise to the right of existing nodes
  const addStageToGraph = (stage, x, y) => {
    let posX = x;
    let posY = y;
    const hasPos = typeof posX === 'number' && typeof posY === 'number';
    if (!hasPos) {
      posX = 40;
      posY = 40;
      const nodes = graph.getNodes();
      if (nodes.length > 0) {
        const positions = nodes.map((n) => n.getPosition());
        const rightmost = positions.reduce((a, b) => (a.x >= b.x ? a : b));
        posX = rightmost.x + gridSize;
        posY = rightmost.y;
        const maxWidth = graph.container?.clientWidth || 800;
        if (posX > maxWidth - gridSize) {
          const maxY = Math.max(...positions.map((p) => p.y));
          posX = 40;
          posY = maxY + gridSize;
        }
      }
    }

    const node = graph.addNode({
      shape: 'stage_node',
      id: stage.id,
      x: posX,
      y: posY,
    });
    return node;
  };

  const updateNodeProps = (stage, node, belongingScene) => {
    const isOrgasm = !!(
      stage.positions &&
      stage.positions.some((p) => p.climax || p.extra?.climax)
    );
    node.prop('stage', stage);
    node.prop('scene', belongingScene);
    node.prop('fixedLen', stage.extra.fixed_len);
    node.prop('isStart', belongingScene && belongingScene.root === stage.id);
    node.prop('isOrgasm', isOrgasm);
    node.prop('isTransition', isTransitionStage(stage));
    node.prop(
      'displayName',
      uniqueStageLabel(stage, belongingScene?.stages || [])
    );
  }

  const saveScene = () => {
    let has_warnings = false;
    let doSave = true;
    if (!activeScene.name) {
      api['error']({
        message: 'Missing Name',
        description: 'Add a short, descriptive name to your scene.',
        placement: 'bottomLeft',
        onClick(evt) {
          const elm = document.getElementById('stageNameInputField');
          elm.focus();
        }
      });
      doSave = false;
    }
    const nodes = graph.getNodes();
    const startNode = nodes.find(node => node.id === activeScene.root);
    if (!startNode) {
      api['warning']({
        message: 'Missing Start Animation',
        description: 'Choose the stage which the scene is supposed to start at.',
        placement: 'bottomLeft'
      });
      has_warnings = true;
    } else {
      const dfsGraph = graph.getSuccessors(startNode);
      if (dfsGraph.length + 1 < nodes.length) {
        api['warning']({
          message: 'Unreachable Stages',
          description: 'Scene contains stages which cannot be reached from the start animation',
          placement: 'bottomLeft'
        });
        has_warnings = true;
      }
    }

    if (!doSave || !edited) {
      return;
    }
    // api['success']({
    //   message: 'Saved Scene',
    //   description: `Scene ${activeScene.name} has successfully been saved.`,
    //   placement: 'bottomLeft'
    // });
    const scene = {
      ...activeScene,
      graph: syncStoredGraphFromCanvas(),
      has_warnings,
    };
    invoke('save_scene', { scene }).then(() => {
      console.log("Saved scene", scene);
      updateActiveScene(scene);
      updateScenes(prev => {
        const w = prev.findIndex(it => it.id === scene.id);
        if (w === -1) {
          prev.push(scene);
        } else {
          prev[w] = scene;
        }
      });
      setEdited(false);
      console.log("Saved Scene", scene);
    });
  }

  const sideBarMenu = [
    makeMenuItem('New Scene', 'add', < PlusOutlined />),
    { type: 'divider' },
    makeMenuItem(`Scenes ${scenes.length ? `(${scenes.length})` : ''}`,
      'animations',
      <FolderOutlined />,
      scenes.map((scene) => {
        console.log(scene);
        return makeMenuItem(
          <Tooltip title={scene.name} mouseEnterDelay={0.5}>
            {scene.name}
          </Tooltip>, scene.id, scene.has_warnings ? <WarningOutlined style={{ color: 'red' }} /> : <ExperimentOutlined style={{ color: 'green' }} />, [
          makeMenuItem("Edit", "editanim_" + scene.id),
          makeMenuItem("Delete", "delanim_" + scene.id, null, null, false, true),
        ]);
      })
    )
  ];

  const onSiderSelect = async ({ key }) => {
    const idx = key.lastIndexOf("_");
    const option = idx == -1 ? key : key.substring(0, idx);
    const id = key.substring(idx + 1);
    const scene = scenes.find(scene => scene.id === id);
    switch (option) {
      case 'add':
        {
          const new_anim = await invoke('create_blank_scene');
          // Register immediately so Clone-to / sidebar can find it even before
          // the user saves from the scene editor.
          try {
            await invoke('save_scene', { scene: new_anim });
            updateScenes((prev) => {
              if (prev.some((s) => s.id === new_anim.id)) return;
              prev.push(new_anim);
            });
          } catch (err) {
            console.error('Failed to register new scene', err);
          }
          setActiveScene(new_anim);
          setShowAreas(true);
          setEdited(true);
        }
        break;
      case 'editanim':
        setActiveScene(scene);
        setShowAreas(true);
        break;
      case 'delanim':
        {
          confirm({
            title: 'Deleting Scene',
            icon: <ExclamationCircleOutlined />,
            content: `Are you sure you want to delete the scene '${scene.name}'?\n\nThis action cannot be undone.`,
            onOk() {
              try {
                invoke('delete_scene', { id });
                updateScenes(prev => prev.filter(scene => scene.id !== id));
                if (activeScene && activeScene.id === id) {
                  updateActiveScene(null);
                  setEdited(false);
                }
              } catch (error) {
                console.log(error);
              }
            },
            onCancel() { },
          });
          break;
        }
      default:
        console.log("Unrecognized option %s", option);
        break;
    }
  }

  const blankStagePosition = () => ({
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

  const blankPositionInfo = () => ({
    sex: { male: true, female: false, futa: false },
    race: 'Human',
    scale: 1.0,
    submissive: false,
    vampire: false,
    dead: false,
    add_cum: 0,
    id: generatePositionId(),
  });

  // Clone-to always keeps the source stage's actor count. Destination
  // PositionInfo slots are taken from the source scene (falling back to the
  // target, then blanks) — never shrink a 3-actor stage into a 1-actor anim.
  const prepareCloneToTarget = (stage, sourceScene, targetScene) => {
    const adaptedStage = structuredClone(stage);
    const target = structuredClone(targetScene);
    const sourceInfos = sourceScene?.positions || [];
    const n = adaptedStage.positions?.length ?? 0;
    const nextInfos = [];
    for (let i = 0; i < n; i++) {
      const fromSource = sourceInfos[i];
      const fromTarget = target.positions?.[i];
      if (fromSource) {
        nextInfos.push({
          ...structuredClone(fromSource),
          id: generatePositionId(),
        });
      } else if (fromTarget) {
        nextInfos.push({
          ...structuredClone(fromTarget),
          id: fromTarget.id || generatePositionId(),
        });
      } else {
        nextInfos.push(blankPositionInfo());
      }
    }
    target.positions = nextInfos;
    return { adaptedStage, target };
  };

  const confirmCloneTo = () => {
    if (!cloneToStage || !cloneToTargetId) return;
    const target =
      (activeScene && activeScene.id === cloneToTargetId && activeScene) ||
      scenes.find((s) => s.id === cloneToTargetId);
    if (!target) {
      api.error({ message: 'Target scene not found', placement: 'bottomLeft' });
      return;
    }
    // Prefer the live source stage from the source scene (actor count may have
    // changed after the modal opened).
    const sourceStage =
      cloneToSourceScene?.stages?.find((s) => s.id === cloneToStage.id) ||
      cloneToStage;
    const { adaptedStage, target: targetWithActors } = prepareCloneToTarget(
      sourceStage,
      cloneToSourceScene,
      target
    );
    invoke('open_stage_editor_from', {
      sceneId: targetWithActors.id,
      positions: targetWithActors.positions || [],
      copyStage: adaptedStage,
      existingStageCount: targetWithActors.stages?.length || 0,
    });
    setCloneToOpen(false);
    setCloneToStage(null);
    setCloneToSourceScene(null);
    setCloneToTargetId(null);
  };

  return (
    <ConfigProvider theme={getAppTheme(isDark)}>
      <Layout hasSider style={{ height: '100vh' }}>
        <PanelGroup
          direction="horizontal"
          autoSaveId="slsb-main-horizontal"
          style={{ height: '100%' }}
        >
          {/* Left Panel */}
          <Panel minSize={10} defaultSize={15} maxSize={50} id="left-panel">
            {contextHolder}
            <Modal
              title="Clone stage to animation"
              open={cloneToOpen}
              onOk={confirmCloneTo}
              onCancel={() => {
                setCloneToOpen(false);
                setCloneToStage(null);
                setCloneToSourceScene(null);
                setCloneToTargetId(null);
              }}
              okButtonProps={{ disabled: !cloneToTargetId }}
              okText="Clone"
              destroyOnClose
            >
              <p style={{ marginBottom: 12 }}>
                Open a copy of this stage in another animation. The cloned stage
                keeps this stage&apos;s actor count; the destination animation
                is expanded to match.
              </p>
              <Select
                style={{ width: '100%' }}
                placeholder="Select animation"
                value={cloneToTargetId}
                onChange={setCloneToTargetId}
                options={(() => {
                  const list = [...scenes];
                  if (
                    activeScene &&
                    !list.some((s) => s.id === activeScene.id)
                  ) {
                    list.push(activeScene);
                  }
                  return list.map((s) => ({
                    value: s.id,
                    label: s.name || s.id || 'Untitled',
                  }));
                })()}
                showSearch
                optionFilterProp="label"
              />
            </Modal>
            <Sider
              className="main-sider"
              collapsible
              collapsed={collapsed}
              onCollapse={(value) => setCollapsed(value)}
              width="100%"
              trigger={null}
            >
              <div className="sider-content">
                <input
                  type="text"
                  placeholder="Package Name"
                  className="sidebar-form"
                  value={packName}
                  onChange={(e) => {
                    const name = e.target.value;
                    setPackName(name);
                    invoke('set_pack_name', { name });
                    setEdited(true);
                  }}
                />
                <input
                  type="text"
                  placeholder="Author Name"
                  className="sidebar-form"
                  value={packAuthor}
                  onChange={(e) => {
                    const author = e.target.value;
                    setPackAuthor(author);
                    invoke('set_pack_author', { author });
                    setEdited(true);
                  }}
                />
                <input
                  type="text"
                  placeholder="Pack Version"
                  className="sidebar-form"
                  value={packVersion}
                  onChange={(e) => {
                    const version = e.target.value;
                    setPackVersion(version);
                    invoke('set_pack_version', { version });
                    setEdited(true);
                  }}
                />
                <Divider id="sidebar-divider" />
                <Menu
                  theme={isDark ? 'dark' : 'light'}
                  mode="inline"
                  selectable={false}
                  items={sideBarMenu}
                  onClick={onSiderSelect}
                />
              </div>
            </Sider>
          </Panel>
          {/* End Left Panel */}

          <PanelResizeHandle className="resize-handle" />

          <Panel>
            <PanelGroup direction="vertical" autoSaveId="slsb-main-vertical">
              <Panel defaultSize={50} style={{}}>
                <PanelGroup
                  direction="horizontal"
                  autoSaveId="slsb-graph-tags-horizontal"
                >
                  {/* Graph Area */}
                  <Panel id="graph-panel">
                    <Layout style={{ height: '100%' }}>
                      <Content>
                        {/* hacky workaround because graph doesnt render nodes if I put the graph interface into a child component zzz */}
                        {/* if (activeScene) ... */}
                        <div
                          className="scene-box"
                          style={{ display: !activeScene ? 'none' : undefined }}
                        >
                          <Card
                            className="graph-editor-field a"
                            style={{
                              height: '100%',
                            }}
                            title={
                              activeScene ? (
                                <Space.Compact style={{ width: '98%' }}>
                                  <div
                                    style={
                                      !edited ? { display: 'none' } : {}
                                    }
                                  >
                                    <Tooltip title={'Unsaved changes'}>
                                      <DiffOutlined
                                        style={{
                                          fontSize: '2em',
                                          color: 'red',
                                        }}
                                      />
                                    </Tooltip>
                                  </div>
                                  <Input
                                    size="large"
                                    maxLength={30}
                                    bordered={false}
                                    id="stageNameInputField"
                                    value={activeScene.name}
                                    onChange={(e) => {
                                      updateActiveScene((prev) => {
                                        prev.name = e.target.value;
                                      });
                                      setEdited(true);
                                    }}
                                    onFocus={(e) => e.target.select()}
                                    placeholder="Scene Name"
                                  />
                                </Space.Compact>
                              ) : (
                                <></>
                              )
                            }
                            extra={
                              <Space.Compact block>
                                <Button
                                  onClick={() => {
                                    const stages = activeScene.stages || [];
                                    invoke('open_stage_editor', {
                                      sceneId: activeScene.id,
                                      positions: activeScene.positions || [],
                                      stage: null,
                                      existingStageCount: stages.length,
                                      templateStage:
                                        stages.length > 0
                                          ? stages[stages.length - 1]
                                          : null,
                                    });
                                  }}
                                >
                                  Add Stage
                                </Button>
                                <Button onClick={saveScene} type="primary">
                                  Store
                                </Button>
                              </Space.Compact>
                            }
                            // bodyStyle={{ height: 'calc(100% - 190px)' }}
                          >
                            <div className="graph-toolbox">
                              <Space
                                className="graph-toolbox-content"
                                size={'small'}
                                align="center"
                              >
                                <Tooltip title="Undo" mouseEnterDelay={0.5}>
                                  <Button
                                    type="text"
                                    size="small"
                                    icon={<DoubleLeftOutlined />}
                                    onClick={() => {
                                      if (graph.canUndo()) graph.undo();
                                    }}
                                  />
                                </Tooltip>
                                <Tooltip title="Redo" mouseEnterDelay={0.5}>
                                  <Button
                                    type="text"
                                    size="small"
                                    icon={<DoubleRightOutlined />}
                                    onClick={() => {
                                      if (graph.canRedo()) graph.redo();
                                    }}
                                  />
                                </Tooltip>
                                <Divider type="vertical" />
                                <Tooltip
                                  title="Center content"
                                  mouseEnterDelay={0.5}
                                >
                                  <Button
                                    type="text"
                                    size="small"
                                    icon={<CompressOutlined />}
                                    onClick={() => graph.centerContent()}
                                  />
                                </Tooltip>
                                <Tooltip
                                  title="Fit to screen"
                                  mouseEnterDelay={0.5}
                                >
                                  <Button
                                    type="text"
                                    size="small"
                                    icon={<PicCenterOutlined />}
                                    onClick={() => graph.zoomToFit()}
                                  />
                                </Tooltip>
                                <Tooltip
                                  title="Arrange navigation layout (primary spanning tree)"
                                  mouseEnterDelay={0.5}
                                >
                                  <Button
                                    type="text"
                                    size="small"
                                    icon={<ApartmentOutlined />}
                                    onClick={() => arrangeStages()}
                                  />
                                </Tooltip>
                                <Tooltip
                                  title="Restore packed positions from scene open (SLSB coords only)"
                                  mouseEnterDelay={0.5}
                                >
                                  <Button
                                    type="text"
                                    size="small"
                                    icon={<UndoOutlined />}
                                    disabled={!layoutSnapshotRef.current}
                                    onClick={() => restorePackedPositions()}
                                  />
                                </Tooltip>
                                <Divider type="vertical" />
                                <Tooltip
                                  title="Collapsed: via-edge labels. Poses/Transitions: full graph with inactive layer dimmed."
                                  mouseEnterDelay={0.5}
                                >
                                  <Segmented
                                    size="small"
                                    value={transitionLayerMode}
                                    onChange={(v) => {
                                      const prev = transitionLayerModeRef.current;
                                      setTransitionLayerMode(v);
                                      transitionLayerModeRef.current = v;
                                      const wasCollapsed = prev === 'collapsed';
                                      const nowCollapsed = v === 'collapsed';
                                      if (wasCollapsed !== nowCollapsed) {
                                        presentationCacheRef.current = null;
                                        if (!nowCollapsed) {
                                          updateActiveScene((draft) => {
                                            if (
                                              disambiguateDuplicateStageNames(
                                                draft.stages || []
                                              )
                                            ) {
                                              setEdited(true);
                                            }
                                          });
                                        }
                                        setTimeout(() => {
                                          rebuildGraphPresentation({
                                            rearrange: false,
                                          });
                                        }, 0);
                                      } else {
                                        queueMicrotask(() => {
                                          applyGraphLayerDim(graph, v);
                                          applyNodeFamilyDim(
                                            graph,
                                            presentationCacheRef.current?.families,
                                            mapFamilyFilterRef.current
                                          );
                                        });
                                      }
                                    }}
                                    options={[
                                      { value: 'collapsed', label: 'Collapsed' },
                                      { value: 'poses', label: 'Poses' },
                                      { value: 'transitions', label: 'Transitions' },
                                    ]}
                                  />
                                </Tooltip>
                                <Divider type="vertical" />
                                <Segmented
                                  size="small"
                                  value={graphWorkMode}
                                  onChange={(v) => applyWorkMode(v)}
                                  options={[
                                    { value: 'browse', label: 'Browse' },
                                    { value: 'edit', label: 'Edit' },
                                  ]}
                                />
                                <Select
                                  size="small"
                                  value={edgeFilterMode}
                                  style={{ width: 140 }}
                                  onChange={(v) => {
                                    setEdgeFilterMode(v);
                                    edgeFilterModeRef.current = v;
                                    queueMicrotask(() => refreshGraphEdgesRef.current?.());
                                  }}
                                  options={[
                                    { value: 'primary', label: 'Edges: Primary' },
                                    { value: 'neighborhood', label: 'Edges: Near' },
                                    { value: 'family', label: 'Edges: Family' },
                                    { value: 'all', label: 'Edges: All' },
                                  ]}
                                />
                                <Select
                                  size="small"
                                  value={mapFamilyFilter}
                                  style={{ width: 160 }}
                                  onChange={(v) => {
                                    setMapFamilyFilter(v);
                                    mapFamilyFilterRef.current = v;
                                    queueMicrotask(() => refreshGraphEdgesRef.current?.());
                                  }}
                                  options={[
                                    { value: 'all', label: 'All families' },
                                    ...familyFilterOptions.map((f) => ({
                                      value: f,
                                      label: f,
                                    })),
                                  ]}
                                />
                                <GraphNodeSearch
                                  stages={activeScene?.stages || []}
                                  onJump={jumpToNode}
                                />
                                <Tooltip title="Toggle navigation outline" mouseEnterDelay={0.5}>
                                  <Button
                                    type={showOutline ? 'primary' : 'text'}
                                    size="small"
                                    icon={<UnorderedListOutlined />}
                                    onClick={() => setShowOutline((v) => !v)}
                                  />
                                </Tooltip>
                                <Tooltip
                                  title="Export graph / connections"
                                  mouseEnterDelay={0.5}
                                >
                                  <Dropdown
                                    menu={{
                                      items: [
                                        {
                                          key: 'svg',
                                          label: 'Export visible SVG',
                                          onClick: () => exportGraphCanvas('svg'),
                                        },
                                        {
                                          key: 'json',
                                          label: 'Export visible layout JSON',
                                          onClick: () => exportGraphCanvas('json'),
                                        },
                                        {
                                          key: 'csv',
                                          label: 'Export all connections CSV',
                                          onClick: () => exportConnectionsTable(),
                                        },
                                      ],
                                    }}
                                    trigger={['click']}
                                  >
                                    <Button
                                      type="text"
                                      size="small"
                                      icon={<DownloadOutlined />}
                                    />
                                  </Dropdown>
                                </Tooltip>
                                <Tooltip
                                  title="Lock canvas"
                                  mouseEnterDelay={0.5}
                                >
                                  <Switch
                                    size="small"
                                    checkedChildren={<PushpinOutlined />}
                                    unCheckedChildren={<DragOutlined />}
                                    onChange={(checked) => {
                                      graph.togglePanning(!checked);
                                    }}
                                  />
                                </Tooltip>
                                <Divider type="vertical" />
                                <Tooltip title="Zoom out" mouseEnterDelay={0.5}>
                                  <Button
                                    type="text"
                                    size="small"
                                    icon={<ZoomOutOutlined />}
                                    onClick={() => {
                                      graph.zoomTo(
                                        graph.zoom() * 0.8,
                                        ZOOM_OPTIONS
                                      );
                                    }}
                                  />
                                </Tooltip>
                                <Tooltip title="Zoom in" mouseEnterDelay={0.5}>
                                  <Button
                                    type="text"
                                    size="small"
                                    icon={<ZoomInOutlined />}
                                    onClick={() => {
                                      graph.zoomTo(
                                        graph.zoom() * 1.2,
                                        ZOOM_OPTIONS
                                      );
                                    }}
                                  />
                                </Tooltip>
                                <Divider type="vertical" />
                                <Tooltip
                                  title="Clear canvas"
                                  mouseEnterDelay={0.5}
                                >
                                  <Button
                                    type="text"
                                    size="small"
                                    danger
                                    icon={<DeleteOutlined />}
                                    onClick={clearGraph}
                                  />
                                </Tooltip>
                              </Space>
                            </div>
                            <div
                              className="graph-container"
                              style={{
                                display: 'flex',
                                flexDirection: 'row',
                                height: '100%',
                                minHeight: 0,
                              }}
                            >
                              {showOutline && (
                                <div
                                  className="graph-outline-host"
                                  style={{
                                    flex: '0 0 240px',
                                    maxWidth: 280,
                                    minWidth: 180,
                                    borderRight: isDark
                                      ? '1px solid #333'
                                      : '1px solid #e8e8e8',
                                    padding: '8px 8px 4px',
                                    overflow: 'hidden',
                                    display: 'flex',
                                    flexDirection: 'column',
                                  }}
                                >
                                  <Typography.Text
                                    strong
                                    style={{ fontSize: 12, marginBottom: 4 }}
                                  >
                                    Navigation
                                  </Typography.Text>
                                  <GraphNavOutline
                                    outline={navOutline}
                                    selectedIds={focusNodeIds}
                                    pathIds={pathIds}
                                    isDark={isDark}
                                    onSelectNode={jumpToNode}
                                  />
                                </div>
                              )}
                              <div
                                id="graph"
                                ref={graphcontainer_ref}
                                className="graph-canvas-host"
                                style={{
                                  flex: '1 1 auto',
                                  minWidth: 0,
                                  height: '100%',
                                }}
                              />
                            </div>
                          </Card>
                        </div>
                        {/* else ... */}
                        <Empty
                          style={activeScene ? { display: 'none' } : {}}
                          className="graph-no-scene-placeholder"
                          image={Empty.PRESENTED_IMAGE_SIMPLE}
                          description={'No scene loaded :('}
                        >
                          <Button
                            type="primary"
                            onClick={() => onSiderSelect({ key: 'add' })}
                          >
                            New Scene
                          </Button>
                        </Empty>
                        {/* endif */}
                      </Content>
                    </Layout>
                  </Panel>
                  {/* End Graph Area */}

                  <PanelResizeHandle className="resize-handle" />
                  {/* Scene Tags and Furniture area */}
                  {showAreas && (
                    <Panel
                      id="sceneTags-panel"
                      minSize={30}
                      defaultSize={30}
                      maxSize={40}
                    >
                      <Card
                        className="sceneTags-attribute-card"
                        bordered={false}
                        title={'Scene Tags'}
                        extra={
                          <Space size={0}>
                            <Tooltip
                              title="Copy scene tags onto every stage (replaces each stage's tags)."
                            >
                              <Button
                                type="text"
                                disabled={
                                  !activeScene ||
                                  !activeScene.stages ||
                                  activeScene.stages.length === 0
                                }
                                onClick={() => {
                                  if (!activeScene?.stages?.length) return;
                                  const copied = [...(activeScene.tags || [])];
                                  updateActiveScene((prev) => {
                                    for (const stage of prev.stages) {
                                      stage.tags = [...copied];
                                    }
                                  });
                                  setEdited(true);
                                }}
                              >
                                Copy to stages
                              </Button>
                            </Tooltip>
                            <Tooltip
                              className="tool-tip"
                              title={
                                'Tags which are shared between all stages in the scene.'
                              }
                            >
                              <Button type="text">Info</Button>
                            </Tooltip>
                          </Space>
                        }
                      >
                        <TagTree
                          tags={activeScene ? activeScene.tags : []}
                          onChange={(tags) => {
                            updateActiveScene((prev) => {
                              prev.tags = tags;
                            });
                            setEdited(true);
                          }}
                          tagsSFW={activeScene ? tagsSFW : []}
                          tagsNSFW={activeScene ? tagsNSFW : []}
                        />
                      </Card>
                      <Card
                        bordered={false}
                        title={'Furniture'}
                        className="furniture-attribute-card"
                        extra={
                          <Tooltip
                            className="tool-tip"
                            title={'Furniture settings for the scene.'}
                          >
                            <Button type="text">Info</Button>
                          </Tooltip>
                        }
                      >
                        <Space size={'large'} direction="vertical">
                          <Select
                            style={{ overflowY: 'auto' }}
                            className="graph-furniture-selection"
                            value={
                              activeScene
                                ? activeScene.furniture.furni_types
                                : []
                            }
                            options={Furnitures}
                            mode="multiple"
                            onSelect={(value) => {
                              if (value === 'None') {
                                updateActiveScene((prev) => {
                                  prev.furniture.furni_types = [value];
                                  return prev;
                                });
                              } else {
                                updateActiveScene((prev) => {
                                  let where =
                                    prev.furniture.furni_types.indexOf('None');
                                  if (where === -1)
                                    prev.furniture.furni_types.push(value);
                                  else
                                    prev.furniture.furni_types[where] = value;
                                  prev.furniture.allow_bed = false;
                                  return prev;
                                });
                              }
                              setEdited(true);
                            }}
                            onDeselect={(value) => {
                              updateActiveScene((prev) => {
                                prev.furniture.furni_types =
                                  prev.furniture.furni_types.filter(
                                    (it) => it !== value
                                  );
                                if (prev.furniture.furni_types.length === 0) {
                                  prev.furniture.furni_types = ['None'];
                                }
                                return prev;
                              });
                              setEdited(true);
                            }}
                          />
                          <Checkbox
                            onChange={(e) => {
                              updateActiveScene((prev) => {
                                prev.furniture.allow_bed = e.target.checked;
                              });
                              setEdited(true);
                            }}
                            checked={
                              activeScene && activeScene.furniture.allow_bed
                            }
                            disabled={
                              activeScene &&
                              !activeScene.furniture.furni_types.includes(
                                'None'
                              )
                            }
                          >
                            Allow Bed
                          </Checkbox>
                          <Input
                            addonBefore="OStim type"
                            placeholder="optional override (e.g. singlebed, wall)"
                            value={
                              (activeScene && activeScene.furniture.ostim_type) || ''
                            }
                            onChange={(e) => {
                              updateActiveScene((prev) => {
                                prev.furniture.ostim_type = e.target.value;
                                return prev;
                              });
                              setEdited(true);
                            }}
                          />
                          <Checkbox
                            onChange={(e) => {
                              updateActiveScene((prev) => {
                                prev.private = e.target.checked;
                              });
                              setEdited(true);
                            }}
                            checked={activeScene && activeScene.private}
                          >
                            Private
                          </Checkbox>
                          <Row gutter={[12, 12]} justify={'space-evenly'}>
                            <Col>
                              <InputNumber
                                addonBefore={'X'}
                                controls
                                decimalSeparator=","
                                precision={1}
                                step={0.1}
                                value={
                                  activeScene
                                    ? activeScene.furniture.offset.x
                                      ? activeScene.furniture.offset.x
                                      : undefined
                                    : undefined
                                }
                                onChange={(e) => {
                                  updateActiveScene((prev) => {
                                    prev.furniture.offset.x = e;
                                  });
                                  setEdited(true);
                                }}
                                placeholder="0.0"
                              />
                            </Col>
                            <Col>
                              <InputNumber
                                addonBefore={'Y'}
                                controls
                                decimalSeparator=","
                                precision={1}
                                step={0.1}
                                value={
                                  activeScene && activeScene.furniture.offset.y
                                    ? activeScene.furniture.offset.y
                                    : undefined
                                }
                                onChange={(e) => {
                                  updateActiveScene((prev) => {
                                    prev.furniture.offset.y = e;
                                  });
                                  setEdited(true);
                                }}
                                placeholder="0.0"
                              />
                            </Col>
                            <Col>
                              <InputNumber
                                addonBefore={'Z'}
                                controls
                                decimalSeparator=","
                                precision={1}
                                step={0.1}
                                value={
                                  activeScene
                                    ? activeScene.furniture.offset.z
                                      ? activeScene.furniture.offset.z
                                      : undefined
                                    : undefined
                                }
                                onChange={(e) => {
                                  updateActiveScene((prev) => {
                                    prev.furniture.offset.z = e;
                                  });
                                  setEdited(true);
                                }}
                                placeholder="0.0"
                              />
                            </Col>
                            <Col>
                              <InputNumber
                                addonBefore={'°'}
                                controls
                                decimalSeparator=","
                                precision={1}
                                step={0.1}
                                min={0.0}
                                max={359.9}
                                value={
                                  (activeScene &&
                                    activeScene.furniture.offset.r) ||
                                  undefined
                                }
                                onChange={(e) => {
                                  updateActiveScene((prev) => {
                                    prev.furniture.offset.r = e;
                                  });
                                  setEdited(true);
                                }}
                                placeholder="0.0"
                              />
                            </Col>
                          </Row>
                        </Space>
                      </Card>
                    </Panel>
                  )}
                  {/* Scene Tags and Furniture area */}
                </PanelGroup>
              </Panel>

              <PanelResizeHandle className="resize-handle-horizontal" />

              {/* Bottom Positions Field */}
              {showAreas && (
                <Panel
                  minSize={15}
                  maxSize={50}
                  id="scenePositions"
                  style={{ minHeight: '150px' }}
                  defaultSize={25}
                >
                  <Card
                    className="sceneTagsPositions-card"
                    bordered={false}
                    title="Scene Positions"
                    extra={
                      <Tooltip
                        className="tool-tip"
                        title={
                          'Position Date shared between all stages in the scene.'
                        }
                      >
                        <Button type="text">Info</Button>
                      </Tooltip>
                    }
                  >
                    <Space direction="horizontal" style={{ width: '100%' }}>
                      <div className="scene-positions-list">
                        {activeScene &&
                        activeScene.positions &&
                        activeScene.positions.length > 0 ? (
                          activeScene.positions.map((pos, idx) => (
                            <Col key={pos.id || idx} span={24}>
                              <ScenePosition
                                position={pos}
                                onChange={(newPos) => {
                                  updateActiveScene((draft) => {
                                    draft.positions[idx] = {
                                      ...newPos,
                                      id: pos.id || generatePositionId(),
                                    };
                                  });
                                  emit('on_position_change', {
                                    sceneId: activeScene.id,
                                    stageId: 0,
                                    positionIdx: idx,
                                    info: { ...newPos },
                                  });
                                  setEdited(true);
                                }}
                              />
                            </Col>
                          ))
                        ) : (
                          <Col
                            span={24}
                            style={{ padding: 12, textAlign: 'center' }}
                          >
                            <div className="scene-positions-empty">
                              No positions yet — use "Add Stage" or add a
                              position from the stage editor.
                            </div>
                          </Col>
                        )}
                      </div>
                    </Space>
                  </Card>
                </Panel>
              )}
              {/* Bottom Positions Field */}
            </PanelGroup>
          </Panel>
        </PanelGroup>
      </Layout>
    </ConfigProvider>
  );
}

export default App;