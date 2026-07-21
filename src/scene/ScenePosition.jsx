import { useState, useEffect } from "react";
import { Card, Space, InputNumber, Tooltip } from "antd";
import { useImmer } from "use-immer";
import CheckboxEx from "../components/CheckboxEx";
import RaceSelect from "../components/RaceSelect";

function ScenePosition({ position, onChange }) {
  const [sex, updateSex] = useImmer(position.sex);
  const [race, setRace] = useState(position.race);
  const [scale, setScale] = useState(
    typeof position.scale === "number" ? position.scale : 1.0
  );
  const [extra, updateExtra] = useImmer({
    submissive: position.submissive,
    vampire: position.vampire,
    dead: position.dead,
  });

  // Keep local state in sync when the parent swaps/reloads this slot
  useEffect(() => {
    updateSex(position.sex);
    setRace(position.race);
    setScale(typeof position.scale === "number" ? position.scale : 1.0);
    updateExtra({
      submissive: position.submissive,
      vampire: position.vampire,
      dead: position.dead,
    });
  }, [position.id, position.race, position.scale, position.sex, position.submissive, position.vampire, position.dead]);

  useEffect(() => {
    onChange({
      ...position,
      sex,
      race,
      scale: typeof scale === "number" ? scale : 1.0,
      submissive: extra.submissive,
      vampire: extra.vampire,
      dead: extra.dead,
    });
    // Intentionally omit onChange/position to avoid feedback loops
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sex, race, scale, extra]);

  return (
    <Card size="small" className="scene-position-card">
      <div className="scene-position-row">
        <div className="scene-position-section">
          <RaceSelect
            race={race}
            onSelect={(e) => {
              if (e !== "Human") {
                updateSex((prev) => {
                  prev.futa = false;
                });
              }
              setRace(e);
            }}
          />
          <Space.Compact>
            <CheckboxEx obj={sex} label={"Male"} attr={"male"} updateFunc={updateSex} />
            <CheckboxEx obj={sex} label={"Female"} attr={"female"} updateFunc={updateSex} />
            <CheckboxEx
              obj={sex}
              label={"Futa"}
              disabled={race !== "Human"}
              attr={"futa"}
              updateFunc={updateSex}
            />
          </Space.Compact>
        </div>

        <div className="scene-position-divider" aria-hidden />

        <div className="scene-position-section">
          <Space.Compact>
            <Tooltip title={"Passive/Taker/Bottom position."}>
              <div>
                <CheckboxEx
                  obj={extra}
                  label={"Submissive"}
                  attr={"submissive"}
                  updateFunc={updateExtra}
                />
              </div>
            </Tooltip>
            <Tooltip title={"Actor is a vampire."}>
              <div>
                <CheckboxEx
                  obj={extra}
                  label={"Vampire"}
                  attr={"vampire"}
                  disabled={race !== "Human"}
                  updateFunc={updateExtra}
                />
              </div>
            </Tooltip>
            <Tooltip title={"Actor is unconscious/dead."}>
              <div>
                <CheckboxEx
                  obj={extra}
                  label={"Unconscious"}
                  attr={"dead"}
                  updateFunc={updateExtra}
                />
              </div>
            </Tooltip>
          </Space.Compact>
        </div>

        <div className="scene-position-divider" aria-hidden />

        <div className="scene-position-section scene-position-scale">
          <Tooltip title="Actor scale factor used by SexLab for this position (typically 1.0).">
            <span className="scene-position-scale-label">Scale</span>
          </Tooltip>
          <InputNumber
            controls
            decimalSeparator="."
            precision={2}
            min={0.01}
            max={2}
            step={0.01}
            value={scale}
            onChange={(e) => {
              setScale(typeof e === "number" ? e : 1.0);
            }}
            placeholder="1.0"
            style={{ width: 96 }}
          />
        </div>
      </div>
    </Card>
  );
}

export default ScenePosition;
