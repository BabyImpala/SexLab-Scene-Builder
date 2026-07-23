import React, { useMemo, useState } from "react";
import { Select, Tag } from 'antd';
import { loadUserTags, rememberUserTags } from '../common/userTags';

function TagTree({
  tags,
  onChange,
  tagsSFW = [],
  tagsNSFW = [],
  ...selectProps
}) {
  const [userTags, setUserTags] = useState(() => loadUserTags());
  const presets = useMemo(
    () => [...tagsSFW, ...tagsNSFW],
    [tagsSFW, tagsNSFW]
  );

  const options = useMemo(() => {
    const groups = [
      {
        label: 'SFW',
        options: tagsSFW.map((tag) => ({ value: tag, label: tag })),
      },
      {
        label: 'NSFW',
        options: tagsNSFW.map((tag) => ({ value: tag, label: tag })),
      },
    ];
    if (userTags.length) {
      groups.push({
        label: 'Yours',
        options: userTags.map((tag) => ({ value: tag, label: tag })),
      });
    }
    return groups;
  }, [tagsSFW, tagsNSFW, userTags]);

  const handleChange = (next) => {
    const cleaned = [];
    const seen = new Set();
    for (const tag of next || []) {
      const trimmed = String(tag ?? '').trim();
      if (!trimmed) continue;
      const key = trimmed.toLowerCase().replace(/\s+/g, '');
      if (seen.has(key)) continue;
      seen.add(key);
      cleaned.push(trimmed);
    }
    setUserTags(rememberUserTags(cleaned, presets));
    onChange(cleaned);
  };

  return (
    <Select
      className="tag-display-field"
      size="large"
      mode="tags"
      showSearch
      allowClear
      placeholder="Search or create tags"
      value={tags}
      onChange={handleChange}
      options={options}
      optionFilterProp="label"
      tokenSeparators={[',']}
      maxTagTextLength={20}
      tagRender={({ label, value, closable, onClose }) => {
        const search = String(value).toLowerCase();
        const color = tagsSFW.find((it) => it.toLowerCase() === search)
          ? 'cyan'
          : tagsNSFW.find((it) => it.toLowerCase() === search)
            ? 'volcano'
            : 'purple';

        const onPreventMouseDown = (evt) => {
          evt.preventDefault();
          evt.stopPropagation();
        };
        return (
          <Tag
            color={color}
            onMouseDown={onPreventMouseDown}
            closable={closable}
            onClose={onClose}
            style={{ margin: 2 }}
          >
            {label}
          </Tag>
        );
      }}
      {...selectProps}
    />
  );
}

export default TagTree;
