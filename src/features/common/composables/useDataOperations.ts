import { ref } from 'vue';
import { read, utils as xlUtils, writeFile } from 'xlsx';
import { Notification, Modal } from '@arco-design/web-vue';
import { debounce as customDebounce } from '@/utils/debounce.js';
import { downloadWithDialog, openDirectory } from '@/utils/downloadWithDialog';
import { exportWithDialog } from '@/utils/exportWithDialog';

/**
 * Shared data operations composable for both EVM and Solana chains.
 * Chain-specific validation (privateKey/address) is provided via options.
 */
export function useDataOperations(options = {}) {
  const {
    data,
    uploadInputRef,
    clearValidationCache,
    validatePrivateKey,
    validateAddress,
    deriveAddress,
  } = options;

  const importProgress = ref(0);
  const importTotal = ref(0);
  const importCompleted = ref(0);
  const showImportProgress = ref(false);
  const importProgressText = ref('');

  function updateImportProgress() {
    if (!showImportProgress.value) return;
    importTotal.value > 0
      ? (importProgress.value = Number((importCompleted.value / importTotal.value).toFixed(4)))
      : (importProgress.value = 0);

    if (importCompleted.value === importTotal.value && importTotal.value > 0) {
      setTimeout(() => { showImportProgress.value = false; }, 1000);
    }
  }

  function exportInvalidData(invalidData) {
    if (invalidData.length === 0) return;
    const wb = xlUtils.book_new();
    const wsData = [
      ['私钥', '地址', '转账数量', '错误原因'],
      ...invalidData.map((item) => [
        item.私钥 || '', item.地址 || '', item.转账数量 || '', item.错误原因 || '',
      ]),
    ];
    const ws = xlUtils.aoa_to_sheet(wsData);
    xlUtils.book_append_sheet(wb, ws, '不合规数据');
    const timestamp = new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19);
    writeFile(wb, `不合规数据_${timestamp}.xlsx`);
  }

  async function processBatchData(batchData) {
    const validItems = [];
    const invalidItems = [];

    for (let i = 0; i < batchData.length; i++) {
      const item = batchData[i];
      const rowNumber = item._originalIndex + 2;
      const privateKey = String(item.私钥 || '').trim();
      const toAddress = String(item.地址 || '').trim();
      const amount = item.转账数量;

      const isPrivateKeyValid = privateKey && validatePrivateKey(privateKey);
      const isAddressValid = toAddress && validateAddress(toAddress);

      if (isPrivateKeyValid && isAddressValid) {
        try {
          const address = deriveAddress ? deriveAddress(privateKey) : privateKey;
          validItems.push({
            key: `transfer_${validItems.length}_${Date.now()}_${Math.random().toString(36).substr(2, 9)}`,
            private_key: privateKey,
            address,
            to_addr: toAddress,
            amount: amount ? String(amount) : '0',
            plat_balance: '',
            coin_balance: '',
            exec_status: '0',
            error_msg: '',
          });
        } catch {
          invalidItems.push({ 私钥: privateKey, 地址: toAddress, 转账数量: amount || '', 错误原因: '私钥无效', 行号: rowNumber });
        }
      } else {
        const errorReasons = [];
        if (!isPrivateKeyValid) errorReasons.push(privateKey ? '私钥格式错误' : '私钥为空');
        if (!isAddressValid) errorReasons.push(toAddress ? '地址格式错误' : '地址为空');
        invalidItems.push({ 私钥: privateKey, 地址: toAddress, 转账数量: amount || '', 错误原因: errorReasons.join('; '), 行号: rowNumber });
      }

      importCompleted.value++;
      updateImportProgress();
      if (i % 10 === 0) await new Promise((resolve) => setTimeout(resolve, 0));
    }
    return { validItems, invalidItems };
  }

  function UploadFile() {
    if (!uploadInputRef.value.files || !uploadInputRef.value.files[0]) return;
    data.value = [];
    clearValidationCache();

    setTimeout(async () => {
      try {
        let file = uploadInputRef.value.files[0];
        let reader = new FileReader();
        reader.readAsArrayBuffer(file);

        reader.onload = async function () {
          try {
            const buffer = reader.result;
            const bytes = new Uint8Array(buffer);
            let binary = '';
            for (let i = 0; i < bytes.byteLength; i++) binary += String.fromCharCode(bytes[i]);

            const wb = read(binary, { type: 'binary' });
            const outdata = xlUtils.sheet_to_json(wb.Sheets[wb.SheetNames[0]]);
            const indexedData = outdata.map((item, index) => ({ ...item, _originalIndex: index }));

            importTotal.value = indexedData.length;
            importCompleted.value = 0;
            importProgress.value = 0;
            importProgressText.value = '正在处理数据...';
            showImportProgress.value = true;

            const allValidData = [];
            const allInvalidData = [];
            const batchSize = 50;
            const totalBatches = Math.ceil(indexedData.length / batchSize);

            for (let i = 0; i < totalBatches; i++) {
              const start = i * batchSize;
              const end = Math.min(start + batchSize, indexedData.length);
              const { validItems, invalidItems } = await processBatchData(indexedData.slice(start, end));
              allValidData.push(...validItems);
              allInvalidData.push(...invalidItems);
              importProgressText.value = '正在处理数据...';
            }

            importProgressText.value = '数据处理完成，正在渲染表格...';
            await new Promise((resolve) => setTimeout(resolve, 100));
            data.value = allValidData.map((item, index) => ({ ...item, key: String(index + 1) }));

            if (allInvalidData.length > 0) {
              exportInvalidData(allInvalidData);
              if (allValidData.length > 0) {
                Notification.warning({ title: '导入完成', content: `成功导入 ${allValidData.length} 条数据，${allInvalidData.length} 条不合规数据已导出到本地文件`, duration: 5000, position: 'topLeft' });
              } else {
                Notification.error({ title: '导入失败', content: `所有数据都不合规，共 ${allInvalidData.length} 条数据已导出到本地文件`, duration: 5000, position: 'topLeft' });
              }
            } else {
              Notification.success({ title: '导入成功！', content: `成功导入 ${allValidData.length} 条数据`, duration: 3000, position: 'topLeft' });
            }
          } catch (error) {
            console.error('文件处理失败:', error);
            Notification.error({ title: '文件处理失败', content: '文件处理过程中发生错误，请检查文件格式是否正确', duration: 5000, position: 'topLeft' });
          } finally {
            if (uploadInputRef.value) uploadInputRef.value.value = '';
          }
        };
        reader.onerror = function () {
          Notification.error({ title: '文件读取失败', content: '文件读取过程中发生错误，请检查文件格式是否正确', duration: 5000, position: 'topLeft' });
        };
      } catch (error) {
        console.error('导入文件失败:', error);
        Notification.error({ title: '导入失败', content: '导入过程中发生错误，请重试', duration: 5000, position: 'topLeft' });
      }
    }, 100);
  }

  function upload() { uploadInputRef.value.click(); }

  const downloadFile = customDebounce(() => {
    downloadWithDialog('import_model.xlsx', '导入模板.xlsx');
  }, 1000);

  async function downloadTemplate() {
    const filePath = await downloadWithDialog('import_model.xlsx', '导入模板.xlsx');
    if (filePath) {
      openDirectory(filePath);
      Notification.success({ content: '模板已保存', duration: 5000, position: 'topLeft' });
    }
  }

  function exportPrivateKeyAddress(dataToExport, options = {}) {
    const { isSelected = false } = options;
    if (dataToExport.length === 0) {
      Notification.warning({ content: isSelected ? '请先选择要导出的数据！' : '当前列表无数据！', position: 'topLeft' });
      return;
    }
    const timestamp = new Date().toISOString().slice(0, 19).replace(/[:-]/g, '');
    const exportData = [['private_key', 'address']];
    dataToExport.forEach((item) => {
      exportData.push([item.private_key || '', item.to_addr || item.address || '']);
    });
    exportWithDialog(exportData, `private_key_address_${timestamp}.xlsx`).then((path) => {
      if (path) {
        openDirectory(path);
        Notification.success({ content: `已导出 ${dataToExport.length} 条私钥地址对`, duration: 4000, position: 'topLeft' });
      }
    });
  }

  function clearData(options = {}) {
    const { startLoading, balanceLoading } = options;
    if (startLoading?.value) { Notification.warning({ content: '请停止或等待转账完成后再清空列表！', position: 'topLeft' }); return; }
    if (balanceLoading?.value) { Notification.warning({ content: '请停止或等待查询完成后再清空列表！', position: 'topLeft' }); return; }
    if (data.value.length === 0) { Notification.warning({ content: '当前列表无数据！', position: 'topLeft' }); return; }

    Modal.confirm({
      title: '确认清空',
      content: '确定要清空所有列表数据吗？此操作不可撤销。',
      onOk: () => {
        data.value = [];
        clearValidationCache();
        if (uploadInputRef.value) uploadInputRef.value.value = '';
        Notification.success({ content: '清空列表成功！', position: 'topLeft' });
      },
    });
  }

  function deleteItem(item, options = {}) {
    const { startLoading } = options;
    if (startLoading?.value) { Notification.warning({ content: '请停止或等待执行完成后再删除数据！', position: 'topLeft' }); return null; }
    return { success: true, key: item.key, privateKey: item.private_key || '' };
  }

  return {
    importProgress, importTotal, importCompleted, showImportProgress, importProgressText,
    updateImportProgress, processBatchData, UploadFile, upload,
    downloadFile, downloadTemplate, exportPrivateKeyAddress, clearData, deleteItem,
  };
}
