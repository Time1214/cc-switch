import { useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import type { SettingsFormState } from "@/hooks/useSettings";
import { AppWindow, MonitorUp, Power, EyeOff, Trash2 } from "lucide-react";
import { ToggleRow } from "@/components/ui/toggle-row";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { settingsApi } from "@/lib/api";

interface WindowSettingsProps {
  settings: SettingsFormState;
  onChange: (updates: Partial<SettingsFormState>) => void;
}

export function WindowSettings({ settings, onChange }: WindowSettingsProps) {
  const { t } = useTranslation();
  const [isClearing, setIsClearing] = useState(false);

  const handleClearVscodeConfig = async () => {
    setIsClearing(true);
    try {
      await settingsApi.clearVscodeClaudeConfig();
      toast.success(
        t("settings.clearVscodeClaudeConfigSuccess", {
          defaultValue: "VS Code Claude 插件配置已清除",
        }),
      );
    } catch (error) {
      toast.error(
        t("settings.clearVscodeClaudeConfigFailed", {
          defaultValue: "清除失败: {{error}}",
          error: (error as Error)?.message ?? String(error),
        }),
      );
    } finally {
      setIsClearing(false);
    }
  };

  return (
    <section className="space-y-4">
      <div className="flex items-center gap-2 pb-2 border-b border-border/40">
        <AppWindow className="h-4 w-4 text-primary" />
        <h3 className="text-sm font-medium">{t("settings.windowBehavior")}</h3>
      </div>

      <div className="space-y-3">
        <ToggleRow
          icon={<Power className="h-4 w-4 text-orange-500" />}
          title={t("settings.launchOnStartup")}
          description={t("settings.launchOnStartupDescription")}
          checked={!!settings.launchOnStartup}
          onCheckedChange={(value) => onChange({ launchOnStartup: value })}
        />

        <ToggleRow
          icon={<EyeOff className="h-4 w-4 text-green-500" />}
          title={t("settings.silentStartup")}
          description={t("settings.silentStartupDescription")}
          checked={!!settings.silentStartup}
          onCheckedChange={(value) => onChange({ silentStartup: value })}
        />

        <ToggleRow
          icon={<MonitorUp className="h-4 w-4 text-purple-500" />}
          title={t("settings.enableClaudePluginIntegration")}
          description={t("settings.enableClaudePluginIntegrationDescription")}
          checked={!!settings.enableClaudePluginIntegration}
          onCheckedChange={(value) =>
            onChange({ enableClaudePluginIntegration: value })
          }
        />

        <div className="space-y-2">
          <ToggleRow
            icon={<MonitorUp className="h-4 w-4 text-blue-500" />}
            title={t("settings.enableVscodeClaudeSync")}
            description={t("settings.enableVscodeClaudeSyncDescription")}
            checked={!!settings.enableVscodeClaudeSync}
            onCheckedChange={(value) =>
              onChange({ enableVscodeClaudeSync: value })
            }
          />
          {settings.enableVscodeClaudeSync && (
            <div className="ml-11 space-y-3 rounded-lg border border-border/40 bg-muted/30 p-3">
              <div className="space-y-1.5">
                <p className="text-xs font-medium text-foreground">
                  {t("settings.vscodeSettingsPath")}
                </p>
                <p className="text-xs text-muted-foreground">
                  {t("settings.vscodeSettingsPathDescription")}
                </p>
                <Input
                  value={settings.vscodeSettingsPath ?? ""}
                  placeholder={t("settings.vscodeSettingsPathPlaceholder")}
                  className="text-xs"
                  onChange={(e) =>
                    onChange({
                      vscodeSettingsPath: e.target.value || undefined,
                    })
                  }
                />
              </div>
              <div className="flex items-center gap-2 pt-1 border-t border-border/30">
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  className="text-xs text-destructive hover:text-destructive"
                  disabled={isClearing}
                  onClick={handleClearVscodeConfig}
                >
                  <Trash2 className="mr-1.5 h-3.5 w-3.5" />
                  {t("settings.clearVscodeClaudeConfig")}
                </Button>
                <p className="text-xs text-muted-foreground">
                  {t("settings.clearVscodeClaudeConfigDescription")}
                </p>
              </div>
            </div>
          )}
        </div>

        <ToggleRow
          icon={<MonitorUp className="h-4 w-4 text-cyan-500" />}
          title={t("settings.skipClaudeOnboarding")}
          description={t("settings.skipClaudeOnboardingDescription")}
          checked={!!settings.skipClaudeOnboarding}
          onCheckedChange={(value) => onChange({ skipClaudeOnboarding: value })}
        />

        <ToggleRow
          icon={<AppWindow className="h-4 w-4 text-blue-500" />}
          title={t("settings.minimizeToTray")}
          description={t("settings.minimizeToTrayDescription")}
          checked={settings.minimizeToTrayOnClose}
          onCheckedChange={(value) =>
            onChange({ minimizeToTrayOnClose: value })
          }
        />
      </div>
    </section>
  );
}
