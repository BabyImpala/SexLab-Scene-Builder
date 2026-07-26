import { useMemo, useState } from 'react';
import {
  Card,
  Input,
  InputNumber,
  Select,
  Space,
  Typography,
  Tooltip,
  Button,
  Empty,
} from 'antd';
import { ostimIconSelectOptions } from '../common/ostimIcons';
import { suggestAssetOptions, rememberAssetValues } from '../common/assetLibrary';

/**
 * Per-destination OStim navigation metadata (description, priority, icon, border).
 * Writes `ostim_nav:{prio}:{dest}:{desc}:{icon}:{border}` tags via onChange.
 */
export default function OstimNavFields({ rows = [], onChange, assetLibrary }) {
  const [iconSearch, setIconSearch] = useState('');
  const iconOptions = useMemo(() => {
    const vanilla = ostimIconSelectOptions();
    const fromLib = suggestAssetOptions(assetLibrary, 'icons').map((value) => ({
      value,
      label: value,
    }));
    const seen = new Set(vanilla.map((o) => String(o.value).toLowerCase()));
    const extra = fromLib.filter((o) => !seen.has(String(o.value).toLowerCase()));
    return [...extra, ...vanilla];
  }, [assetLibrary]);

  const patchRow = (index, patch) => {
    const next = rows.map((r, i) => (i === index ? { ...r, ...patch } : r));
    onChange?.(next);
  };

  if (!rows.length) {
    return (
      <Empty
        image={Empty.PRESENTED_IMAGE_SIMPLE}
        description="Link this stage to others on the graph to edit OStim icons and priorities."
        style={{ margin: '12px 0' }}
      />
    );
  }

  return (
    <Space direction="vertical" size={12} style={{ width: '100%' }}>
      {rows.map((row, i) => (
        <Card
          key={`${row.dest}-${i}`}
          size="small"
          title={
            <Space size={8} wrap>
              <Typography.Text strong>{row.label || row.dest}</Typography.Text>
              {row.external ? (
                <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                  (external)
                </Typography.Text>
              ) : null}
              <Typography.Text
                type="secondary"
                copyable={{ text: row.dest }}
                style={{ fontSize: 11 }}
              >
                {row.dest}
              </Typography.Text>
            </Space>
          }
        >
          <Space direction="vertical" size={8} style={{ width: '100%' }}>
            <div>
              <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                Description
              </Typography.Text>
              <Input
                value={row.description || ''}
                maxLength={80}
                placeholder="Player-facing label (e.g. Kiss)"
                onChange={(e) =>
                  patchRow(i, {
                    description: e.target.value.replace(/:/g, ' '),
                  })
                }
              />
            </div>
            <Space wrap style={{ width: '100%' }} size={12}>
              <div style={{ minWidth: 120 }}>
                <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                  Priority
                </Typography.Text>
                <InputNumber
                  style={{ width: '100%' }}
                  value={row.priority}
                  step={100}
                  onChange={(v) =>
                    patchRow(i, {
                      priority: Number.isFinite(v) ? v : 1000,
                    })
                  }
                />
              </div>
              <div style={{ minWidth: 140, flex: 1 }}>
                <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                  Border (hex)
                </Typography.Text>
                <Input
                  value={row.border || ''}
                  placeholder="e.g. ff6699"
                  maxLength={8}
                  onChange={(e) =>
                    patchRow(i, {
                      border: e.target.value.replace(/^#/, '').replace(/:/g, ''),
                    })
                  }
                  addonBefore="#"
                />
              </div>
            </Space>
            <div>
              <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                Icon{' '}
                <Tooltip title="Path under Interface/OStim/icons/ (without .dds). Pick a vanilla icon or type a custom pack path.">
                  <Button type="link" size="small" style={{ padding: 0 }}>
                    ?
                  </Button>
                </Tooltip>
              </Typography.Text>
              <Select
                showSearch
                allowClear
                style={{ width: '100%' }}
                placeholder="OStim/…"
                value={row.icon || undefined}
                options={iconOptions}
                searchValue={iconSearch}
                onSearch={setIconSearch}
                onChange={(v) => {
                  setIconSearch('');
                  if (v) rememberAssetValues('icons', v);
                  patchRow(i, { icon: v || '' });
                }}
                filterOption={(input, option) => {
                  const q = String(input || '').toLowerCase();
                  if (!q) return true;
                  const val = String(option?.value || '').toLowerCase();
                  const lab = String(option?.label || '').toLowerCase();
                  return val.includes(q) || lab.includes(q);
                }}
                popupMatchSelectWidth={false}
                notFoundContent={
                  iconSearch.trim() ? (
                    <Button
                      type="link"
                      onMouseDown={(e) => e.preventDefault()}
                      onClick={() => {
                        const v = iconSearch.trim();
                        setIconSearch('');
                        rememberAssetValues('icons', v);
                        patchRow(i, { icon: v });
                      }}
                    >
                      Use custom “{iconSearch.trim()}”
                    </Button>
                  ) : (
                    'No icons'
                  )
                }
              />
            </div>
          </Space>
        </Card>
      ))}
    </Space>
  );
}
