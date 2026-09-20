<script setup lang="ts">
import { ref, onMounted } from 'vue';
import { useAgentStore } from '../stores/agentStore';
import {
  NCard,
  NDescriptions,
  NDescriptionsItem,
  NButton,
  NSwitch,
  NSelect,
  NInputNumber,
  NTag,
  useMessage,
} from 'naive-ui';
import { getVersion } from '@tauri-apps/api/app';
import { setAutostart, initAutostartDefault } from '../services/autostart';
import { restartAgent, getSystemSettings, saveSystemSettings, openExternalUrl } from '../services/tauri';
import { checkForUpdate } from '../services/update';
import type { SystemSettings } from '../types/task';

const agentStore = useAgentStore();
const message = useMessage();

const autostartLoading = ref(false);
const autostartActive = ref(true);
const restartingAgent = ref(false);

const settingsLoading = ref(false);
const retentionPreset = ref<string>('7');
const customDays = ref<number>(7);

const appVersion = ref('-');
const checkingUpdate = ref(false);
const updateChecked = ref(false);
const noReleases = ref(false);
const hasUpdate = ref(false);
const latestVersion = ref('');
const releaseNotes = ref('');
const releaseUrl = ref('');
const releasePublishedAt = ref('');
const updateError = ref('');

const retentionPresetOptions = [
  { label: '保留 3 天', value: '3' },
  { label: '保留 7 天 (系统默认推荐)', value: '7' },
  { label: '保留 14 天', value: '14' },
  { label: '保留 30 天', value: '30' },
  { label: '保留 90 天', value: '90' },
  { label: '自定义天数', value: 'custom' },
  { label: '永久保留 (从不自动清理)', value: 'permanent' },
];

onMounted(async () => {
  try {
    autostartActive.value = await initAutostartDefault();
  } catch (e) {
    autostartActive.value = false;
  }

  try {
    const sysSettings = await getSystemSettings();
    if (sysSettings.default_log_retention.mode === 'Permanent') {
      retentionPreset.value = 'permanent';
    } else {
      const days = sysSettings.default_log_retention.days;
      if ([3, 7, 14, 30, 90].includes(days)) {
        retentionPreset.value = String(days);
      } else {
        retentionPreset.value = 'custom';
        customDays.value = days;
      }
    }
  } catch (e) {
    console.error('Failed to load system settings:', e);
  }

  try {
    appVersion.value = await getVersion();
  } catch (e) {
    console.error('Failed to get app version:', e);
  }
  await handleCheckUpdate(true);
});

async function handleSaveRetentionSettings() {
  settingsLoading.value = true;
  try {
    let payload: SystemSettings;
    if (retentionPreset.value === 'permanent') {
      payload = { default_log_retention: { mode: 'Permanent' } };
    } else if (retentionPreset.value === 'custom') {
      const days = Math.max(1, Math.floor(customDays.value || 7));
      payload = { default_log_retention: { mode: 'KeepDays', days } };
    } else {
      const days = parseInt(retentionPreset.value, 10);
      payload = { default_log_retention: { mode: 'KeepDays', days } };
    }
    await saveSystemSettings(payload);
    message.success('日志保留策略已更新');
  } catch (e: any) {
    message.error('保存设置失败: ' + (e?.message || e));
  } finally {
    settingsLoading.value = false;
  }
}

async function handleCheckUpdate(quiet = false) {
  checkingUpdate.value = true;
  updateError.value = '';
  try {
    const result = await checkForUpdate(appVersion.value);
    updateChecked.value = true;
    noReleases.value = result.noReleases;
    hasUpdate.value = result.hasUpdate;
    latestVersion.value = result.latest || appVersion.value;
    releaseNotes.value = ((result.release && result.release.body) || '').slice(0, 300);
    releaseUrl.value = (result.release && result.release.html_url) || '';
    releasePublishedAt.value = (result.release && result.release.published_at) || '';
    if (!quiet) {
      if (result.noReleases) {
        message.info('GitHub 上暂无已发布的版本');
      } else if (result.hasUpdate) {
        message.info(`发现新版本 ${result.latest}`);
      } else {
        message.success('当前已是最新版本');
      }
    }
  } catch (e: any) {
    updateError.value = e?.message || '检查更新失败';
    if (!quiet) {
      message.error('检查更新失败: ' + updateError.value);
    }
  } finally {
    checkingUpdate.value = false;
  }
}

async function handleOpenRelease() {
  if (!releaseUrl.value) return;
  try {
    await openExternalUrl(releaseUrl.value);
  } catch (e: any) {
    message.error('无法打开浏览器: ' + (e?.message || e));
  }
}

async function handleRestartAgent() {
  restartingAgent.value = true;
  try {
    await restartAgent();
    message.success('Agent 守护进程已成功重启');
    await agentStore.fetchStatus();
  } catch (e: any) {
    message.error('重启守护进程失败: ' + (e?.message || e));
  } finally {
    restartingAgent.value = false;
  }
}

async function handleToggleAutostart(value: boolean) {
  autostartLoading.value = true;
  try {
    await setAutostart(value);
    autostartActive.value = value;
    message.success(value ? '已开启开机自启动' : '已关闭开机自启动');
  } catch (e: any) {
    message.error(`设置开机自启失败: ${e?.message || e}`);
    autostartActive.value = !value;
  } finally {
    autostartLoading.value = false;
  }
}
</script>

<template>
  <div class="max-w-3xl space-y-6">
    <div>
      <h1 class="text-xl font-bold">系统设置与状态</h1>
      <p class="text-xs text-slate-500 dark:text-zinc-400 mt-1">管理系统启动偏好及查看 easyJob 守护进程健康指标</p>
    </div>

    <!-- 启动偏好配置 -->
    <NCard title="系统与启动偏好" size="small">
      <div class="flex items-center justify-between py-1">
        <div class="space-y-1">
          <div class="text-sm font-medium text-slate-800 dark:text-zinc-200">开机自启动</div>
          <div class="text-xs text-slate-500 dark:text-zinc-400">
            开机时在后台静默运行 easyJob 并最小化到系统托盘，自动守护并按时调度各项定时任务
          </div>
        </div>
        <NSwitch
          v-model:value="autostartActive"
          :loading="autostartLoading"
          @update:value="handleToggleAutostart"
        />
      </div>
    </NCard>

    <!-- 默认日志保留策略配置 -->
    <NCard title="默认日志保留策略" size="small">
      <div class="space-y-4 py-1">
        <div class="flex items-center justify-between">
          <div class="space-y-1">
            <div class="text-sm font-medium text-slate-800 dark:text-zinc-200">全局默认保留时长</div>
            <div class="text-xs text-slate-500 dark:text-zinc-400">
              各任务默认继承此策略。任务执行完成后自动在后台清理过期历史日志及输出
            </div>
          </div>
          <div class="flex items-center gap-2">
            <NSelect
              v-model:value="retentionPreset"
              :options="retentionPresetOptions"
              size="small"
              style="width: 220px;"
            />
            <NInputNumber
              v-if="retentionPreset === 'custom'"
              v-model:value="customDays"
              size="small"
              :min="1"
              style="width: 110px;"
            >
              <template #suffix>天</template>
            </NInputNumber>
            <NButton
              size="small"
              type="primary"
              :loading="settingsLoading"
              @click="handleSaveRetentionSettings"
            >
              保存
            </NButton>
          </div>
        </div>
      </div>
    </NCard>

    <!-- 版本与更新 -->
    <NCard title="版本与更新" size="small">
      <NDescriptions label-placement="left" :column="2" bordered size="small">
        <NDescriptionsItem label="当前版本">
          <span class="font-medium">{{ appVersion === '-' ? '-' : `v${appVersion}` }}</span>
        </NDescriptionsItem>
        <NDescriptionsItem label="最新版本">
          <span class="font-medium">{{ updateChecked ? (noReleases ? '暂无发布' : latestVersion) : '-' }}</span>
          <NTag
            v-if="updateChecked && !updateError && !noReleases"
            size="small"
            :type="hasUpdate ? 'warning' : 'success'"
            style="margin-left: 8px;"
          >
            {{ hasUpdate ? '有新版本' : '已是最新' }}
          </NTag>
        </NDescriptionsItem>
        <NDescriptionsItem v-if="updateChecked && hasUpdate && releasePublishedAt" label="发布时间">
          <span class="font-medium">{{ releasePublishedAt }}</span>
        </NDescriptionsItem>
        <NDescriptionsItem v-if="updateError" label="检查结果">
          <span class="text-rose-500">{{ updateError }}</span>
        </NDescriptionsItem>
      </NDescriptions>

      <div
        v-if="updateChecked && hasUpdate && releaseNotes"
        class="mt-3 rounded border border-slate-200 bg-slate-50 p-3 text-xs text-slate-600 dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-300"
      >
        <div class="mb-1 font-medium">更新内容</div>
        <pre class="whitespace-pre-wrap break-words font-sans">{{ releaseNotes }}</pre>
      </div>

      <template #action>
        <div class="flex items-center gap-3">
          <NButton
            v-if="updateChecked && hasUpdate && releaseUrl"
            size="small"
            type="primary"
            @click="handleOpenRelease"
          >
            前往下载新版本
          </NButton>
          <NButton size="small" secondary :loading="checkingUpdate" @click="handleCheckUpdate(false)">
            检查更新
          </NButton>
        </div>
      </template>
    </NCard>

    <!-- Agent 守护进程状态（横向展示） -->
    <NCard title="Agent 守护进程状态" size="small">
      <NDescriptions label-placement="left" :column="2" bordered size="small">
        <NDescriptionsItem label="连接状态">
          <span :class="agentStore.isConnected ? 'text-emerald-500 font-semibold' : 'text-rose-500 font-semibold'">
            {{ agentStore.isConnected ? '运行中 (Connected)' : '未连接 (Disconnected)' }}
          </span>
        </NDescriptionsItem>
        <NDescriptionsItem label="运行时长">
          <span class="font-medium">{{ agentStore.status?.uptime_secs != null ? `${agentStore.status.uptime_secs} s` : '0 s' }}</span>
        </NDescriptionsItem>
        <NDescriptionsItem label="当前活跃任务">
          <span class="font-medium">{{ agentStore.status?.active_tasks ?? 0 }} 个</span>
        </NDescriptionsItem>
        <NDescriptionsItem label="并发执行中">
          <span class="font-medium">{{ agentStore.status?.running_executions ?? 0 }} 个</span>
        </NDescriptionsItem>
      </NDescriptions>

      <template #action>
        <div class="flex items-center gap-3">
          <NButton size="small" secondary :loading="restartingAgent" @click="handleRestartAgent">
            重启守护进程
          </NButton>
          <NButton size="small" secondary @click="agentStore.fetchStatus()">
            重新检测连接
          </NButton>
        </div>
      </template>
    </NCard>
  </div>
</template>
