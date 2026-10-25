use std::collections::HashMap;
use std::time::Duration;
use std::process::Command;

struct TestResult {
    name: String,
    result: String,
    reason: Option<String>,
}

struct TestExecutor {
    test_list: Vec<String>,
    prerequisites: HashMap<String, Vec<String>>,
    repeat_count: HashMap<String, usize>,
    global_timeout: Duration,
    vm_context: HashMap<String, String>,
    test_result_store: HashMap<String, TestResult>,
}

impl TestExecutor {
    fn new(test_list: Vec<String>, prerequisites: HashMap<String, Vec<String>>, repeat_count: HashMap<String, usize>, global_timeout: Duration, vm_context: HashMap<String, String>, test_result_store: HashMap<String, TestResult>) -> Self {
        TestExecutor {
            test_list,
            prerequisites,
            repeat_count,
            global_timeout,
            vm_context,
            test_result_store,
        }
    }

    fn execute(&mut self) {
        for test in &self.test_list {
            if self.should_skip(test) {
                self.store_result(test, "Skipped", Some("Prerequisite test failed".to_string()));
                continue;
            }

            for i in 0..=self.repeat_count.get(test).unwrap_or(&0) {
                let test_name = if *self.repeat_count.get(test).unwrap_or(&0) > 0 {
                    format!("{}_{}", test, i + 1)
                } else {
                    test.clone()
                };

                let result = self.run_test(&test_name);
                self.store_result(&test_name, &result, None);
            }
        }
    }

    fn should_skip(&self, test: &str) -> bool {
        if let Some(prerequisites) = self.prerequisites.get(test) {
            for prereq in prerequisites {
                if let Some(result) = self.test_result_store.get(prereq) {
                    if result.result != "Passed" {
                        return true;
                    }
                }
            }
        }
        false
    }

    fn run_test(&self, test_name: &str) -> String {
        let timeout = self.global_timeout;
        let vm_context = self.vm_context.clone();
        let command = format!("cargo test --test {} --timeout {}", test_name, timeout.as_secs());

        match Command::new("sh").arg("-c").arg(command).output() {
            Ok(output) => {
                if output.status.success() {
                    "Passed".to_string()
                } else {
                    "Failed".to_string()
                }
            }
            Err(_) => "Failed".to_string(),
        }
    }

    fn store_result(&mut self, test_name: &str, result: &str, reason: Option<String>) {
        self.test_result_store.insert(test_name.to_string(), TestResult {
            name: test_name.to_string(),
            result: result.to_string(),
            reason,
        });
    }
}

fn main() {
    let test_list = vec!["test1", "test2", "test3"];
    let prerequisites = HashMap::from([
        ("test2".to_string(), vec!["test1".to_string()]),
        ("test3".to_string(), vec!["test2".to_string()]),
    ]);
    let repeat_count = HashMap::from([
        ("test2".to_string(), 2),
    ]);
    let global_timeout = Duration::from_secs(60);
    let vm_context = HashMap::from([
        ("vm_name".to_string(), "vm1".to_string()),
    ]);
    let test_result_store = HashMap::new();

    let mut executor = TestExecutor::new(test_list, prerequisites, repeat_count, global_timeout, vm_context, test_result_store);
    executor.execute();
}
