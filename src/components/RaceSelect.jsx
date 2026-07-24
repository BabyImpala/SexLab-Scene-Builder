import { useState, useEffect } from "react";
import { Select } from "antd";
import { invoke } from "@tauri-apps/api/core"

function RaceSelect({ race, onSelect, ...raceSelectProps }) {
  const [raceKeys, setRaceKeys] = useState([]);

  useEffect(() => {
    invoke('get_race_keys')
      .then((result) => {
        setRaceKeys(Array.isArray(result) ? result : []);
      })
      .catch(() => {
        setRaceKeys([]);
      });
  }, []);

  return (
    <Select
      className="position-race-select"
      value={race}
      showSearch
      placeholder="Race"
      optionFilterProp="children"
      filterOption={(input, option) =>
        (option?.label ?? '').includes(input)
      }
      filterSort={(optionA, optionB) =>
        (optionA?.label ?? '')
          .toLowerCase()
          .localeCompare((optionB?.label ?? '').toLowerCase())
      }
      options={raceKeys.map((key) => {
        return { value: key, label: key };
      })}
      onSelect={(value) => {
        onSelect(value);
      }}
      {...raceSelectProps}
    />
  );
}

export default RaceSelect;
