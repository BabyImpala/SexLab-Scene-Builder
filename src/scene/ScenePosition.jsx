import { Card, Space, InputNumber, Tooltip } from "antd";
import { produce } from "immer";
import CheckboxEx from "../components/CheckboxEx";
import RaceSelect from "../components/RaceSelect";

function ScenePosition({ position, onChange }) {
  const sex = position.sex || {};
  const race = position.race;
  const scale = typeof position.scale === "number" ? position.scale : 1.0;
  const extra = {
    submissive: position.submissive,
    vampire: position.vampire,
    dead: position.dead,
  };

  const push = (patch) => {
    onChange({
      ...position,
      sex,
      race,
      scale,
      submissive: extra.submissive,
      vampire: extra.vampire,
      dead: extra.dead,
      ...patch,
    });
  };

  return (
    <Card size="small" className="scene-position-card">
      <div className="scene-position-row">
        <div className="scene-position-section">
          <RaceSelect
            race={race}
            onSelect={(e) => {
              if (e !== "Human") {
                push({
                  race: e,
                  sex: produce(sex, (draft) => {
                    draft.futa = false;
                  }),
                });
              } else {
                push({ race: e });
              }
            }}
          />
          <Space.Compact>
            <CheckboxEx
              obj={sex}
              label={"Male"}
              attr={"male"}
              updateFunc={(recipe) => push({ sex: produce(sex, recipe) })}
            />
            <CheckboxEx
              obj={sex}
              label={"Female"}
              attr={"female"}
              updateFunc={(recipe) => push({ sex: produce(sex, recipe) })}
            />
            <CheckboxEx
              obj={sex}
              label={"Futa"}
              disabled={race !== "Human"}
              attr={"futa"}
              updateFunc={(recipe) => push({ sex: produce(sex, recipe) })}
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
                  updateFunc={(recipe) => {
                    const next = produce(extra, recipe);
                    push({
                      submissive: next.submissive,
                      vampire: next.vampire,
                      dead: next.dead,
                    });
                  }}
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
                  updateFunc={(recipe) => {
                    const next = produce(extra, recipe);
                    push({
                      submissive: next.submissive,
                      vampire: next.vampire,
                      dead: next.dead,
                    });
                  }}
                />
              </div>
            </Tooltip>
            <Tooltip title={"Actor is unconscious/dead."}>
              <div>
                <CheckboxEx
                  obj={extra}
                  label={"Unconscious"}
                  attr={"dead"}
                  updateFunc={(recipe) => {
                    const next = produce(extra, recipe);
                    push({
                      submissive: next.submissive,
                      vampire: next.vampire,
                      dead: next.dead,
                    });
                  }}
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
              push({ scale: typeof e === "number" ? e : 1.0 });
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
