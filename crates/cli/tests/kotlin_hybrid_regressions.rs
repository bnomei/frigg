#![allow(clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use frigg::domain::EvidenceChannel;
use frigg::searcher::{SearchHybridExecutionOutput, SearchHybridQuery, TextSearcher};
use frigg::settings::FriggConfig;

const ENTRYPOINT_WITNESSES: &[&str] = &[
    "app/src/main/java/com/example/android/architecture/blueprints/todoapp/TodoActivity.kt",
    "app/src/main/java/com/example/android/architecture/blueprints/todoapp/TodoApplication.kt",
    "app/src/main/java/com/example/android/architecture/blueprints/todoapp/TodoNavGraph.kt",
    "app/src/main/java/com/example/android/architecture/blueprints/todoapp/TodoNavigation.kt",
];

const UI_MODULE_WITNESSES: &[&str] = &[
    "app/src/main/java/com/example/android/architecture/blueprints/todoapp/addedittask/AddEditTaskScreen.kt",
    "app/src/main/java/com/example/android/architecture/blueprints/todoapp/addedittask/AddEditTaskViewModel.kt",
    "app/src/main/java/com/example/android/architecture/blueprints/todoapp/statistics/StatisticsScreen.kt",
    "app/src/main/java/com/example/android/architecture/blueprints/todoapp/tasks/TasksScreen.kt",
];

fn temp_workspace_root(test_name: &str) -> PathBuf {
    let nanos_since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "frigg-kotlin-hybrid-regressions-{test_name}-{}-{nanos_since_epoch}",
        std::process::id()
    ))
}

fn cleanup_workspace_root(workspace_root: &Path) {
    if workspace_root.exists() {
        fs::remove_dir_all(workspace_root).expect("temporary workspace should be removable");
    }
}

fn prepare_workspace(workspace_root: &Path, files: &[(&str, &str)]) {
    for (relative_path, contents) in files {
        let absolute_path = workspace_root.join(relative_path);
        if let Some(parent) = absolute_path.parent() {
            fs::create_dir_all(parent).expect("failed to create temporary fixture directory");
        }
        fs::write(&absolute_path, contents).expect("failed to seed temporary fixture source");
    }
}

fn searcher_for_workspace_root(workspace_root: &Path) -> TextSearcher {
    let config = FriggConfig::from_workspace_roots(vec![workspace_root.to_path_buf()])
        .expect("workspace root must produce valid config");
    TextSearcher::new(config)
}

fn top_paths(searcher: &TextSearcher, query: &str, limit: usize) -> Vec<String> {
    search_output(searcher, query, limit)
        .matches
        .into_iter()
        .map(|matched| matched.document.path)
        .collect()
}

fn search_output(
    searcher: &TextSearcher,
    query: &str,
    limit: usize,
) -> SearchHybridExecutionOutput {
    searcher
        .search_hybrid(SearchHybridQuery {
            query: query.to_owned(),
            limit,
            weights: Default::default(),
            semantic: Some(false),
        })
        .expect("search_hybrid should succeed for kotlin regression harness")
}

fn seed_architecture_samples_wave_fixture(workspace_root: &Path) {
    prepare_workspace(
        workspace_root,
        &[
            (
                ".github/workflows/build_test.yaml",
                "name: Build\njobs:\n  build:\n    steps:\n      - run: ./gradlew assembleDebug\n",
            ),
            (
                "app/src/main/AndroidManifest.xml",
                "<manifest package=\"com.example.todo\"><application android:name=\".TodoApplication\"><activity android:name=\".TodoActivity\" /></application></manifest>\n",
            ),
            (
                "app/build.gradle.kts",
                "plugins { id(\"com.android.application\") }\nandroid { namespace = \"com.example.todo\" }\n// config build gradle android\n",
            ),
            (
                "build.gradle.kts",
                "plugins { id(\"com.android.application\") version \"8.0.0\" apply false }\n",
            ),
            ("gradle.properties", "android.useAndroidX=true\n# config\n"),
            (
                "settings.gradle.kts",
                "rootProject.name = \"architecture-samples\"\ninclude(\":app\")\n",
            ),
            ("gradle/init.gradle.kts", "initscript {\n    // config\n}\n"),
            (
                "app/src/main/java/com/example/android/architecture/blueprints/todoapp/TodoActivity.kt",
                "class TodoActivity : ComponentActivity()\n",
            ),
            (
                "app/src/main/java/com/example/android/architecture/blueprints/todoapp/TodoApplication.kt",
                "class TodoApplication : Application()\n",
            ),
            (
                "app/src/main/java/com/example/android/architecture/blueprints/todoapp/TodoNavGraph.kt",
                "object TodoNavGraph\n",
            ),
            (
                "app/src/main/java/com/example/android/architecture/blueprints/todoapp/TodoNavigation.kt",
                "class TodoNavigation\n",
            ),
            (
                "app/src/main/java/com/example/android/architecture/blueprints/todoapp/util/CoroutinesUtils.kt",
                "object CoroutinesUtils {\n    fun ioDispatcher() = Unit\n}\n",
            ),
            (
                "app/src/main/java/com/example/android/architecture/blueprints/todoapp/addedittask/AddEditTaskScreen.kt",
                "fun AddEditTaskScreen() {}\n",
            ),
            (
                "app/src/main/java/com/example/android/architecture/blueprints/todoapp/addedittask/AddEditTaskViewModel.kt",
                "class AddEditTaskViewModel\n",
            ),
            (
                "app/src/main/java/com/example/android/architecture/blueprints/todoapp/statistics/StatisticsScreen.kt",
                "fun StatisticsScreen() {}\n",
            ),
            (
                "app/src/main/java/com/example/android/architecture/blueprints/todoapp/tasks/TasksScreen.kt",
                "fun TasksScreen() {}\n",
            ),
            (
                "app/src/test/java/com/example/android/architecture/blueprints/todoapp/addedittask/AddEditTaskViewModelTest.kt",
                "class AddEditTaskViewModelTest\n",
            ),
            (
                "app/src/test/java/com/example/android/architecture/blueprints/todoapp/data/DefaultTaskRepositoryTest.kt",
                "class DefaultTaskRepositoryTest\n",
            ),
            (
                "app/src/test/java/com/example/android/architecture/blueprints/todoapp/statistics/StatisticsUtilsTest.kt",
                "class StatisticsUtilsTest\n",
            ),
            (
                "app/src/test/java/com/example/android/architecture/blueprints/todoapp/statistics/StatisticsViewModelTest.kt",
                "class StatisticsViewModelTest\n",
            ),
            (
                "app/src/test/java/com/example/android/architecture/blueprints/todoapp/taskdetail/TaskDetailViewModelTest.kt",
                "class TaskDetailViewModelTest\n",
            ),
            (
                "app/src/test/java/com/example/android/architecture/blueprints/todoapp/tasks/TasksViewModelTest.kt",
                "class TasksViewModelTest\n",
            ),
            (
                "app/src/androidTest/java/com/example/android/architecture/blueprints/todoapp/addedittask/AddEditTaskScreenTest.kt",
                "class AddEditTaskScreenTest\n",
            ),
            (
                "app/src/androidTest/java/com/example/android/architecture/blueprints/todoapp/data/source/local/TaskDaoTest.kt",
                "class TaskDaoTest\n",
            ),
            (
                "app/src/androidTest/java/com/example/android/architecture/blueprints/todoapp/statistics/StatisticsScreenTest.kt",
                "class StatisticsScreenTest\n",
            ),
            (
                "app/src/androidTest/java/com/example/android/architecture/blueprints/todoapp/taskdetail/TaskDetailScreenTest.kt",
                "class TaskDetailScreenTest\n",
            ),
            (
                "app/src/androidTest/java/com/example/android/architecture/blueprints/todoapp/tasks/AppNavigationTest.kt",
                "class AppNavigationTest\n",
            ),
            (
                "app/src/androidTest/java/com/example/android/architecture/blueprints/todoapp/tasks/TasksScreenTest.kt",
                "class TasksScreenTest\n",
            ),
            (
                "app/src/androidTest/java/com/example/android/architecture/blueprints/todoapp/tasks/TasksTest.kt",
                "class TasksTest\n",
            ),
            ("app/proguard-rules.pro", "# keep rules\n"),
            ("app/src/main/res/drawable/logo_no_fill.png", "asset\n"),
            ("app/src/main/res/drawable/trash_icon.png", "asset\n"),
            (
                "app/src/main/res/drawable/drawer_item_color.xml",
                "<shape />\n",
            ),
            ("app/src/main/res/drawable/ic_add.xml", "<vector />\n"),
            (
                "app/src/main/res/drawable/ic_assignment_turned_in_24dp.xml",
                "<vector />\n",
            ),
            (
                "app/src/main/res/drawable/ic_check_circle_96dp.xml",
                "<vector />\n",
            ),
            ("app/src/main/res/drawable/ic_done.xml", "<vector />\n"),
            ("app/src/main/res/drawable/ic_edit.xml", "<vector />\n"),
            ("renovate.json", "{ \"extends\": [\"config:base\"] }\n"),
            ("LICENSE", "Apache-2.0\n"),
        ],
    );
}

fn seed_smsforwarder_ui_wave_fixture(workspace_root: &Path) {
    prepare_workspace(
        workspace_root,
        &[
            (
                "app/src/main/res/values-en/strings.xml",
                "<resources><string name=\"lock_screen\">Lock screen</string></resources>\n",
            ),
            (
                "app/src/main/AndroidManifest.xml",
                "<manifest package=\"cn.ppps.forwarder\"><application android:name=\".App\" /></manifest>\n",
            ),
            (
                "app/src/main/kotlin/cn/ppps/forwarder/workers/LockScreenWorker.kt",
                "class LockScreenWorker\n",
            ),
            (
                "app/src/main/kotlin/cn/ppps/forwarder/entity/condition/LockScreenSetting.kt",
                "data class LockScreenSetting(val enabled: Boolean)\n",
            ),
            (
                "app/src/main/kotlin/cn/ppps/forwarder/activity/SplashActivity.kt",
                "class SplashActivity\n",
            ),
            (
                "app/src/main/kotlin/cn/ppps/forwarder/fragment/condition/LockScreenFragment.kt",
                "class LockScreenFragment\n",
            ),
            (
                "app/src/main/kotlin/cn/ppps/forwarder/receiver/LockScreenReceiver.kt",
                "class LockScreenReceiver\n",
            ),
            (
                "app/src/main/kotlin/cn/ppps/forwarder/utils/ProximitySensorScreenHelper.kt",
                "object ProximitySensorScreenHelper\n",
            ),
            (
                "app/src/main/res/layout/fragment_tasks_condition_lock_screen.xml",
                "<LinearLayout />\n",
            ),
            (
                "app/src/main/kotlin/cn/ppps/forwarder/database/viewmodel/BaseViewModelFactory.kt",
                "class BaseViewModelFactory\n",
            ),
            (
                "app/src/main/kotlin/cn/ppps/forwarder/database/viewmodel/FrpcViewModel.kt",
                "class FrpcViewModel\n",
            ),
            (
                "app/src/main/kotlin/cn/ppps/forwarder/database/viewmodel/LogsViewModel.kt",
                "class LogsViewModel\n",
            ),
            (
                "app/src/main/kotlin/cn/ppps/forwarder/database/viewmodel/MsgViewModel.kt",
                "class MsgViewModel\n",
            ),
            (
                "app/src/main/kotlin/cn/ppps/forwarder/database/viewmodel/RuleViewModel.kt",
                "class RuleViewModel\n",
            ),
            (
                "app/src/main/kotlin/cn/ppps/forwarder/database/viewmodel/SenderViewModel.kt",
                "class SenderViewModel\n",
            ),
            (
                "app/src/main/kotlin/cn/ppps/forwarder/database/viewmodel/TaskViewModel.kt",
                "class TaskViewModel\n",
            ),
        ],
    );
}

#[test]
fn kotlin_entrypoint_queries_recover_android_startup_witnesses_under_resource_crowding() {
    let workspace_root = temp_workspace_root("android-entrypoints");
    seed_architecture_samples_wave_fixture(&workspace_root);

    let ranked_paths = top_paths(
        &searcher_for_workspace_root(&workspace_root),
        "entry point bootstrap app activity navigation main cli",
        14,
    );

    assert!(
        ranked_paths
            .iter()
            .take(14)
            .any(|path| ENTRYPOINT_WITNESSES.contains(&path.as_str())),
        "saved Kotlin entrypoint queries should recover at least one Android startup witness: {ranked_paths:?}"
    );
    assert!(
        ranked_paths
            .iter()
            .take(14)
            .any(|path| path == "app/src/main/AndroidManifest.xml"),
        "entrypoint queries should keep Android runtime config visible: {ranked_paths:?}"
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn kotlin_config_queries_keep_gradle_config_and_surface_android_entrypoints() {
    let workspace_root = temp_workspace_root("android-config");
    seed_architecture_samples_wave_fixture(&workspace_root);

    let ranked_paths = top_paths(&searcher_for_workspace_root(&workspace_root), "config", 14);

    assert!(
        ranked_paths
            .iter()
            .take(14)
            .any(|path| matches!(path.as_str(), "app/build.gradle.kts" | "gradle.properties")),
        "config queries should retain Gradle config witnesses near the top: {ranked_paths:?}"
    );
    assert!(
        ranked_paths
            .iter()
            .take(14)
            .any(|path| ENTRYPOINT_WITNESSES.contains(&path.as_str())),
        "config queries should still recover at least one Android startup witness: {ranked_paths:?}"
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn kotlin_test_queries_keep_ui_module_companions_visible() {
    let workspace_root = temp_workspace_root("android-tests-ui");
    seed_architecture_samples_wave_fixture(&workspace_root);

    let ranked_paths = top_paths(
        &searcher_for_workspace_root(&workspace_root),
        "tests fixtures integration add edit task dao",
        14,
    );

    assert!(
        ranked_paths
            .iter()
            .take(14)
            .any(|path| UI_MODULE_WITNESSES.contains(&path.as_str())),
        "test-focused queries should keep a Kotlin UI module companion visible: {ranked_paths:?}"
    );
    assert!(
        ranked_paths.iter().take(14).any(|path| {
            matches!(
                path.as_str(),
                "app/src/test/java/com/example/android/architecture/blueprints/todoapp/addedittask/AddEditTaskViewModelTest.kt"
                    | "app/src/androidTest/java/com/example/android/architecture/blueprints/todoapp/addedittask/AddEditTaskScreenTest.kt"
                    | "app/src/androidTest/java/com/example/android/architecture/blueprints/todoapp/data/source/local/TaskDaoTest.kt"
            )
        }),
        "test-focused queries should retain Kotlin test witnesses: {ranked_paths:?}"
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn kotlin_ui_module_queries_keep_test_companions_visible() {
    let workspace_root = temp_workspace_root("android-ui-modules");
    seed_architecture_samples_wave_fixture(&workspace_root);

    let ranked_paths = top_paths(
        &searcher_for_workspace_root(&workspace_root),
        "tests ui compose screen feature integration add edit task dao",
        14,
    );

    assert!(
        ranked_paths
            .iter()
            .take(14)
            .any(|path| UI_MODULE_WITNESSES.contains(&path.as_str())),
        "ui-module queries should recover Kotlin screen and view-model witnesses: {ranked_paths:?}"
    );
    assert!(
        ranked_paths.iter().take(14).any(|path| {
            matches!(
                path.as_str(),
                "app/src/test/java/com/example/android/architecture/blueprints/todoapp/addedittask/AddEditTaskViewModelTest.kt"
                    | "app/src/androidTest/java/com/example/android/architecture/blueprints/todoapp/addedittask/AddEditTaskScreenTest.kt"
                    | "app/src/androidTest/java/com/example/android/architecture/blueprints/todoapp/data/source/local/TaskDaoTest.kt"
            )
        }),
        "ui-module queries should keep Kotlin test companions visible: {ranked_paths:?}"
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn kotlin_saved_smsforwarder_ui_queries_recover_viewmodel_witnesses_under_android_noise() {
    let workspace_root = temp_workspace_root("smsforwarder-ui-wave");
    seed_smsforwarder_ui_wave_fixture(&workspace_root);

    let output = search_output(
        &searcher_for_workspace_root(&workspace_root),
        "tests ui compose screen feature integration",
        9,
    );
    let ranked_paths = output
        .matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect::<Vec<_>>();
    let witness_paths = output
        .channel_results
        .iter()
        .find(|result| result.channel == EvidenceChannel::PathSurfaceWitness)
        .map(|result| {
            result
                .hits
                .iter()
                .map(|hit| hit.document.path.as_str())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    assert!(
        ranked_paths.iter().take(9).any(|path| {
            matches!(
                *path,
                "app/src/main/kotlin/cn/ppps/forwarder/database/viewmodel/FrpcViewModel.kt"
                    | "app/src/main/kotlin/cn/ppps/forwarder/database/viewmodel/LogsViewModel.kt"
                    | "app/src/main/kotlin/cn/ppps/forwarder/database/viewmodel/MsgViewModel.kt"
                    | "app/src/main/kotlin/cn/ppps/forwarder/database/viewmodel/RuleViewModel.kt"
                    | "app/src/main/kotlin/cn/ppps/forwarder/database/viewmodel/SenderViewModel.kt"
                    | "app/src/main/kotlin/cn/ppps/forwarder/database/viewmodel/TaskViewModel.kt"
            )
        }),
        "saved SmsForwarder tests-and-ui queries should recover at least one ViewModel witness; ranked_paths={ranked_paths:?}; witness_paths={witness_paths:?}"
    );

    cleanup_workspace_root(&workspace_root);
}
