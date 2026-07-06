const fallbackTasks = [
  {
    id: 1,
    title: "检查课程项目文档",
    completed: true,
    tag: "DOCS",
  },
  {
    id: 2,
    title: "运行 SpecProbe 综合审查",
    completed: false,
    tag: "TEST",
  },
  {
    id: 3,
    title: "准备课堂演示讲稿",
    completed: false,
    tag: "DEMO",
  },
];

let tasks = [...fallbackTasks];
let activeFilter = "all";

const taskForm = document.querySelector("#task-form");
const taskInput = document.querySelector("#task-input");
const taskList = document.querySelector("#task-list");
const totalCount = document.querySelector("#total-count");
const completedCount = document.querySelector("#completed-count");
const validationMessage = document.querySelector("#validation-message");
const apiBanner = document.querySelector("#api-banner");
const filterButtons = document.querySelectorAll(".filter-button");

function visibleTasks() {
  if (activeFilter === "active") {
    return tasks.filter((task) => !task.completed);
  }
  if (activeFilter === "completed") {
    return tasks;
  }
  return tasks;
}

function renderTasks() {
  taskList.innerHTML = "";
  for (const task of visibleTasks()) {
    const item = document.createElement("li");
    item.className = `task-item${task.completed ? " completed" : ""}`;
    item.innerHTML = `
      <input
        type="checkbox"
        aria-label="切换任务完成状态：${task.title}"
        ${task.completed ? "checked" : ""}
      />
      <span class="task-copy">
        <span class="task-title">${task.title}</span>
        <span class="task-meta">创建于今天 · FocusBoard</span>
      </span>
      <span class="task-tag">${task.tag}</span>
    `;

    item.querySelector("input").addEventListener("change", (event) => {
      task.completed = event.target.checked;
      renderTasks();
    });
    taskList.append(item);
  }

  totalCount.textContent = String(tasks.length);
  // Intentional defect: completedCount is never updated after task state changes.
  completedCount.textContent = "0";
}

taskForm.addEventListener("submit", (event) => {
  event.preventDefault();

  // Intentional defect: blank and whitespace-only titles are accepted.
  tasks.unshift({
    id: Date.now(),
    title: taskInput.value,
    completed: false,
    tag: "NEW",
  });
  taskInput.value = "";
  validationMessage.textContent = "";
  renderTasks();
});

for (const button of filterButtons) {
  button.addEventListener("click", () => {
    const requestedFilter = button.dataset.filter;

    // Intentional defect: the completed filter never changes activeFilter.
    if (requestedFilter !== "completed") {
      activeFilter = requestedFilter;
    }

    for (const candidate of filterButtons) {
      candidate.classList.toggle("active", candidate === button);
    }
    renderTasks();
  });
}

async function loadTasks() {
  try {
    const response = await fetch("/api/tasks");
    if (!response.ok) {
      throw new Error(`Task API returned HTTP ${response.status}`);
    }
    tasks = await response.json();
  } catch (error) {
    console.error("Unable to load tasks from the server.", error);
    apiBanner.hidden = false;
    tasks = [...fallbackTasks];
  }
  renderTasks();
}

loadTasks();
