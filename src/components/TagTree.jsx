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
  const [searchValue, setSearchValue] = useState('');
  const presets = useMemo(
    () => [...tagsSFW, ...tagsNSFW],
    [tagsSFW, tagsNSFW]
  );
  const presetByKey = useMemo(
    () => new Map(presets.map((tag) => [tagKey(tag), tag])),
    [presets]
  );
  const presetKeys = useMemo(
    () => new Set(presetByKey.keys()),
    [presetByKey]
  );

  const canonicalize = (tag) => {
    const trimmed = String(tag ?? '').trim();
    if (!trimmed) return '';
    return presetByKey.get(tagKey(trimmed)) ?? trimmed;
  };

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

  const pendingCreate = useMemo(() => {
    const trimmed = searchValue.trim();
    if (!trimmed) return null;
    const key = tagKey(trimmed);
    if (presetKeys.has(key)) return null;
    if (yoursOptions.some((tag) => tagKey(tag) === key)) return null;
    return trimmed;
  }, [searchValue, presetKeys, yoursOptions]);

  const options = useMemo(() => {
    const yours = pendingCreate
      ? [...yoursOptions, pendingCreate]
      : yoursOptions;
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
    if (yours.length) {
      groups.push({
        label: 'Yours',
        options: yours.map((tag) => ({ value: tag, label: tag })),
      });
    }
    return groups;
  }, [tagsSFW, tagsNSFW, yoursOptions, pendingCreate]);

  const canonicalValue = useMemo(
    () => (tags || []).map(canonicalize).filter(Boolean),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [tags, presetByKey]
  );

  const commitTags = (next) => {
    const cleaned = [];
    const seen = new Set();
    for (const tag of next || []) {
      const canonical = canonicalize(tag);
      if (!canonical) continue;
      const key = tagKey(canonical);
      if (seen.has(key)) continue;
      seen.add(key);
      cleaned.push(canonical);
    }
    setUserTags(rememberUserTags(cleaned, presets));
    setSearchValue('');
    onChange(cleaned);
  };

  const tryCreateFromSearch = () => {
    const trimmed = searchValue.trim();
    if (!trimmed) return false;
    const canonical = canonicalize(trimmed);
    if (canonicalValue.some((tag) => tagKey(tag) === tagKey(canonical))) {
      setSearchValue('');
      return true;
    }
    commitTags([...canonicalValue, canonical]);
    return true;
  };

  return (
    <Select
      className="tag-display-field"
      size="large"
      mode="multiple"
      showSearch
      allowClear
      placeholder="Search or create tags"
      value={canonicalValue}
      searchValue={searchValue}
      onSearch={setSearchValue}
      onChange={commitTags}
      options={options}
      optionFilterProp="label"
      maxTagTextLength={20}
      onInputKeyDown={(evt) => {
        if (evt.key === ',') {
          evt.preventDefault();
          tryCreateFromSearch();
          return;
        }
        if (evt.key === 'Enter' && pendingCreate) {
          // Prefer creating the pending custom tag over selecting a partial match.
          evt.preventDefault();
          tryCreateFromSearch();
        }
      }}
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
