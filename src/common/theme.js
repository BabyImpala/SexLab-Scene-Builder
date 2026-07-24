import { theme } from 'antd';

export function getAppTheme(isDark) {
  return {
    algorithm: isDark ? theme.darkAlgorithm : theme.defaultAlgorithm,
    token: isDark
      ? {
          colorBgBase: '#141414',
          colorBgLayout: '#141414',
          colorBorder: 'rgba(255, 255, 255, 0.18)',
          colorBorderSecondary: 'rgba(255, 255, 255, 0.12)',
          borderRadius: 6,
          // #1677ff reads neon on charcoal
          colorPrimary: '#5b9bd5',
          colorPrimaryHover: '#7eb0df',
          colorPrimaryActive: '#4a87c0',
          colorInfo: '#8bb8e8',
          colorLink: '#8bb8e8',
          colorLinkHover: '#a8cdf0',
          controlOutline: 'rgba(91, 155, 213, 0.25)',
          colorBgTextHover: 'rgba(255, 255, 255, 0.08)',
          colorBgTextActive: 'rgba(255, 255, 255, 0.14)',
        }
      : {
          borderRadius: 6,
          // keep white cards distinct on grey shell
          colorBorder: 'rgba(33, 35, 48, 0.28)',
          colorBorderSecondary: 'rgba(33, 35, 48, 0.16)',
        },
    components: {
      Layout: isDark
        ? {
            colorBgHeader: '#141414',
            colorBgBody: '#141414',
            colorBgSider: '#141414',
            colorBgTrigger: '#1f1f1f',
          }
        : {
            colorBgHeader: '#ffffff',
            colorBgSider: '#ffffff',
            colorBgTrigger: '#f5f5f5',
          },
      Menu: isDark
        ? {
            itemBg: '#141414',
            subMenuItemBg: '#1f1f1f',
            menuSubMenuBg: '#141414',
            horizontalItemSelectedColor: 'rgba(255, 255, 255, 0.85)',
            horizontalItemSelectedBg: 'transparent',
            itemSelectedColor: 'rgba(255, 255, 255, 0.85)',
            itemSelectedBg: 'rgba(255, 255, 255, 0.08)',
          }
        : {
            itemColor: 'rgba(0, 0, 0, 0.88)',
            horizontalItemSelectedColor: 'rgba(0, 0, 0, 0.88)',
            horizontalItemSelectedBg: 'transparent',
          },
      Collapse: isDark
        ? {}
        : {
            headerBg: '#ffffff',
            contentBg: '#ffffff',
            colorBorder: 'rgba(33, 35, 48, 0.22)',
          },
      Card: isDark
        ? {}
        : {
            colorBorderSecondary: 'rgba(33, 35, 48, 0.22)',
          },
      Tabs: isDark
        ? {}
        : {
            cardBg: 'rgba(255, 255, 255, 0.85)',
          },
    },
  };
}
