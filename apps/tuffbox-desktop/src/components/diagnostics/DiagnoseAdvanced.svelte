<script lang="ts">
  import DiagnoseAdvancedToolbar from "./DiagnoseAdvancedToolbar.svelte";
  import DiagnoseTriagePanels from "./DiagnoseTriagePanels.svelte";
  import DiagnoseGroupTestPanel from "./DiagnoseGroupTestPanel.svelte";
  import DiagnoseConflictsJars from "./DiagnoseConflictsJars.svelte";
  import DiagnoseToolsResults from "./DiagnoseToolsResults.svelte";
  import DiagnosePerformancePanel from "./DiagnosePerformancePanel.svelte";

  // Advanced is intentionally a dumb composition layer. State and side
  // effects stay in the orchestration component; this file owns only the
  // advanced surface and its wiring.
  let {
    projectPath, aiLoading, sessionOk, sharingLog, currentLogText, supportBusy,
    planning, oreLoading, duplicateLoading, unifyLoading, wrongLoaderLoading,
    duplicateJarLoading, authorBusy, timings, aiPrompt = $bindable(), aiShowPrompt = $bindable(),
    runAiExplain, shareCurrentLog, exportSupportPack, copyCurrentLog, openFolder, openSubdir,
    createFixPlan, scanOreGen, scanDuplicateItems, generateUnify, detectWrongLoaderMods,
    detectDuplicateModJars, onOpenAuthorForm, onOpenAiSettings,
    signalGroups, recentSnapshots, suspected, crashMcreator, crashClassFinder,
    classQuery = $bindable(), classBusy, classResults, dependentResults,
    analysisToolsOpen = $bindable(), disablingModId, bisectMods,
    onJumpLine, onDisableMod, onUpdateMod, onToggleBisect, onFindClass,
    onFindDependents, onOpenSnapshots,
    groupTest, groupTestActive, groupTestStatus, groupTestPhaseKey,
    groupTestBusy, groupTestAuto = $bindable(), launching,
    onStartGroupTest, onGroupTestLaunch, onReportGroupTest, onCancelGroupTest,
    graphDiagnostics, duplicateJarGroups, wrongLoaderJars, fixingIdx,
    duplicateJarFixing, wrongLoaderFixing, onFixMissingDependency, onFixDeduplicate,
    onKeepOneDuplicateJar, onDisableWrongJar, onRemoveWrongJar,
    plan, selectedFixOption = $bindable(), applying, onApplyPlan,
    authorOpen = $bindable(), authorId = $bindable(), authorSolution = $bindable(),
    authorSymptoms = $bindable(), authorSuspected = $bindable(), authorActionsJson = $bindable(),
    authorNotes = $bindable(), authorExportPreview, authorMessage, onSaveAuthor,
    onCopyAuthorExport, onOpenAuthorFolder,
  }: any = $props();
</script>

<div class="dx-advanced panel">
        <DiagnoseAdvancedToolbar
          projectPath={projectPath}
          aiLoading={aiLoading}
          sessionOk={sessionOk}
          sharingLog={sharingLog}
          currentLogText={currentLogText}
          supportBusy={supportBusy}
          planning={planning}
          oreLoading={oreLoading}
          duplicateLoading={duplicateLoading}
          unifyLoading={unifyLoading}
          wrongLoaderLoading={wrongLoaderLoading}
          duplicateJarLoading={duplicateJarLoading}
          authorBusy={authorBusy}
          aiPrompt={aiPrompt}
          bind:aiShowPrompt
          runAiExplain={() => void runAiExplain()}
          shareCurrentLog={shareCurrentLog}
          exportSupportPack={exportSupportPack}
          copyCurrentLog={copyCurrentLog}
          openFolder={openFolder}
          openSubdir={openSubdir}
          createFixPlan={() => void createFixPlan()}
          scanOreGen={() => void scanOreGen()}
          scanDuplicateItems={() => void scanDuplicateItems()}
          generateUnify={() => void generateUnify()}
          detectWrongLoaderMods={() => void detectWrongLoaderMods()}
          detectDuplicateModJars={() => void detectDuplicateModJars()}
          openAuthorForm={() => void onOpenAuthorForm()}
          openAiSettings={onOpenAiSettings}
        />
        <DiagnosePerformancePanel timings={timings} />

        <DiagnoseTriagePanels
          signalGroups={[]}
          sections={[]}
          suspected={suspected}
          recentSnapshots={recentSnapshots ?? []}
          mcreatorMods={crashMcreator}
          classFinderResults={crashClassFinder}
          bind:classQuery
          classBusy={classBusy}
          classResults={classResults}
          dependentResults={dependentResults}
          bind:toolsOpen={analysisToolsOpen}
          disablingModId={disablingModId}
          bisectMods={bisectMods}
          worldCoords={null}
          memoryHint={null}
          cascadingBanner={null}
          sourceHint=""
          onJumpLine={onJumpLine}
          onDisableMod={onDisableMod}
          onUpdateMod={onUpdateMod}
          onToggleBisect={onToggleBisect}
          onFindClass={onFindClass}
          onFindDependents={onFindDependents}
          onOpenSnapshots={onOpenSnapshots}
        />

        <DiagnoseGroupTestPanel
          session={groupTest}
          suspects={bisectMods}
          active={groupTestActive}
          status={groupTestStatus(groupTest)}
          phaseKey={groupTestPhaseKey(groupTest)}
          busy={groupTestBusy}
          bind:auto={groupTestAuto}
          projectPath={projectPath}
          launching={launching}
          onStart={onStartGroupTest}
          onTest={onGroupTestLaunch}
          onReport={onReportGroupTest}
          onCancel={onCancelGroupTest}
        />

        <DiagnoseConflictsJars
          graphDiagnostics={graphDiagnostics}
          duplicateJarGroups={duplicateJarGroups}
          wrongLoaderJars={wrongLoaderJars}
          fixingIdx={fixingIdx}
          duplicateJarFixing={duplicateJarFixing}
          wrongLoaderFixing={wrongLoaderFixing}
          onFixMissingDependency={onFixMissingDependency}
          onFixDeduplicate={onFixDeduplicate}
          onKeepOneDuplicateJar={onKeepOneDuplicateJar}
          onDisableWrongJar={onDisableWrongJar}
          onRemoveWrongJar={onRemoveWrongJar}
        />

        <DiagnoseToolsResults
          plan={plan}
          aiPrompt={aiPrompt}
          bind:aiShowPrompt
          bind:selectedOption={selectedFixOption}
          applying={applying}
          onApplyPlan={() => void onApplyPlan()}
          bind:authorOpen
          bind:authorId
          bind:authorSolution
          bind:authorSymptoms
          bind:authorSuspected
          bind:authorActionsJson
          bind:authorNotes
          authorBusy={authorBusy}
          authorExportPreview={authorExportPreview}
          authorMessage={authorMessage}
          onSaveAuthor={() => void onSaveAuthor()}
          onCopyAuthorExport={() => void onCopyAuthorExport()}
          onOpenAuthorFolder={() => void onOpenAuthorFolder()}
        />
</div>

<style>
  .dx-advanced {
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 14px;
  }
  .dx-advanced.panel {
    background: var(--bg-secondary);
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-lg);
  }
  .dx-advanced :global(.tools-group) {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: center;
  }
  .dx-advanced :global(.tools-label) {
    width: 100%;
    font-size: 12px;
    font-weight: 800;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-muted);
  }
</style>
