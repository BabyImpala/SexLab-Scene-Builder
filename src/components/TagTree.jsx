import React, { useMemo, useState } from "react";
import { Select, Tag } from 'antd';
import { loadUserTags, rememberUserTags } from '../common/userTags';

function tagKey(tag) {
  return String(tag).toLowerCase().replace(/\s+/g, '');
}

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
  const presetKeys = useMemo(
    () => new Set(presets.map(tagKey)),
    [presets]
  );

  // Keep every custom value in the Yours group so Ant Design mode="tags"
  // does not invent orphan top-level options that pile up until remount.
  const yoursOptions = useMemo(() => {
    const byKey = new Map();
    for (const tag of userTags) {
      const trimmed = String(tag ?? '').trim();
      if (!trimmed || presetKeys.has(tagKey(trimmed))) continue;
      byKey.set(tagKey(trimmed), trimmed);
    }
    for (const tag of tags || []) {
      const trimmed = String(tag ?? '').trim();
      if (!trimmed || presetKeys.has(tagKey(trimmed))) continue;
      byKey.set(tagKey(trimmed), trimmed);
    }
    return [...byKey.values()].sort((a, b) => a.localeCompare(b));
  }, [userTags, tags, presetKeys]);

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
    if (yoursOptions.length) {
      groups.push({
        label: 'Yours',
        options: yoursOptions.map((tag) => ({ value: tag, label: tag })),
      });
    }
    return groups;
  }, [tagsSFW, tagsNSFW, yoursOptions]);

  const handleChange = (next) => {
    const cleaned = [];
    const seen = new Set();
    for (const tag of next || []) {
      const trimmed = String(tag ?? '').trim();
      if (!trimmed) continue;
      const key = tagKey(trimmed);
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
