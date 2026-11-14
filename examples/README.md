# Examples

Every image is built from the `.fitd` file linked above it. The tests rebuild the showcase and docs images and fail if they are stale.

## Language

Radial maps, layered flows, macros, includes, and a user library.

### [fitd_language.fitd](fitd_language.fitd)

fitd described in fitd: one of each kind of statement, in a small diagram.

![fitd_language](out/fitd_language.png)

### [compiler.fitd](compiler.fitd)

The fitd pipeline itself, as a layered flowchart with layout constraints.

![compiler](out/compiler.png)

### [coffee.fitd](coffee.fitd)

From cherry to cup: a radial map whose center fitd picks, colored by a custom palette.

![coffee](out/coffee.png)

### [group_theory.fitd](group_theory.fitd)

Introductory group theory: cyclic and permutation groups, cosets and Lagrange, isomorphisms and products.

![group_theory](out/group_theory.png)

### [oncall.fitd](library/oncall.fitd)

On-call flow built from a user library beside this file.

![oncall](out/oncall.png)

## Showcase

Several standard-library modules in one figure.

### [01-incident-review.fitd](showcase/01-incident-review.fitd)

Incident review: arch (the system), time (the outage), flow (the response) in one page.

![01-incident-review](showcase/out/01-incident-review.png)

### [02-model-confusion.fitd](showcase/02-model-confusion.fitd)

A results page: model diagram, confusion matrix, and a Venn of error overlap together.

![02-model-confusion](showcase/out/02-model-confusion.png)

### [03-domain-model.fitd](showcase/03-domain-model.fitd)

A domain model: UML classes, inheritance and composition, persisted into an arch store.

![03-domain-model](showcase/out/03-domain-model.png)

### [04-triage.fitd](showcase/04-triage.fitd)

A triage guide: a decision tree whose leaves hand off to flowchart steps.

![04-triage](showcase/out/04-triage.png)

## Gallery

One figure per common academic diagram type.

### [01-flowchart.fitd](gallery/01-flowchart.fitd)

Flowchart (DocFigure, Nickerson: tight genre). std/flow gives the role colours.

![01-flowchart](gallery/out/01-flowchart.png)

### [02-architecture.fitd](gallery/02-architecture.fitd)

Architecture / block diagram (Nickerson: 212 of 983, the largest genre; DocFigure block diagram).

![02-architecture](gallery/out/02-architecture.png)

### [03-neural-network.fitd](gallery/03-neural-network.fitd)

Neural-network architecture (ACL-Fig: neural networks, architecture diagram).

![03-neural-network](gallery/out/03-neural-network.png)

### [04-org-chart.fitd](gallery/04-org-chart.fitd)

Tree / hierarchy, here an org chart (DocFigure tree diagram; ACL-Fig trees).

![04-org-chart](gallery/out/04-org-chart.png)

### [05-syntax-tree.fitd](gallery/05-syntax-tree.fitd)

Syntax tree (ACL-Fig: NLP grammar examples, trees).

![05-syntax-tree](gallery/out/05-syntax-tree.png)

### [06-decision-tree.fitd](gallery/06-decision-tree.fitd)

Decision tree (ML papers, clinical guidelines).

![06-decision-tree](gallery/out/06-decision-tree.png)

### [07-network.fitd](gallery/07-network.fitd)

Network graph (Lohse et al.: networks; ACL-Fig: graph). Radial layout, no library needed.

![07-network](gallery/out/07-network.png)

### [08-confusion-matrix.fitd](gallery/08-confusion-matrix.fitd)

Confusion matrix / heat map (DocFigure: confusion matrix, heat map; ACL-Fig: confusion matrix).

![08-confusion-matrix](gallery/out/08-confusion-matrix.png)

### [09-table.fitd](gallery/09-table.fitd)

Results table (DocFigure, ACL-Fig: tables).

![09-table](gallery/out/09-table.png)

### [10-research-model.fitd](gallery/10-research-model.fitd)

Research model with hypotheses (Nickerson: models, 101 of 983).

![10-research-model](gallery/out/10-research-model.png)

### [11-data-flow.fitd](gallery/11-data-flow.fitd)

Data-flow diagram (Nickerson: flow, 80 of 983).

![11-data-flow](gallery/out/11-data-flow.png)

### [12-layer-stack.fitd](gallery/12-layer-stack.fitd)

Layer / stack diagram (Nickerson: layers).

![12-layer-stack](gallery/out/12-layer-stack.png)

### [13-cycle.fitd](gallery/13-cycle.fitd)

Stage cycle (Nickerson: research or method stages drawn as cycles). A back link closes the loop.

![13-cycle](gallery/out/13-cycle.png)

### [14-uml-class.fitd](gallery/14-uml-class.fitd)

UML class diagram (Reggio et al.: most used UML diagram; Nickerson: most common UML type).

![14-uml-class](gallery/out/14-uml-class.png)

### [15-state-machine.fitd](gallery/15-state-machine.fitd)

UML state machine (Reggio et al.: state machine among the core UML diagrams).

![15-state-machine](gallery/out/15-state-machine.png)

### [16-er.fitd](gallery/16-er.fitd)

Entity-relationship diagram (Nickerson: ERD, static structure).

![16-er](gallery/out/16-er.png)

### [17-timeline.fitd](gallery/17-timeline.fitd)

Timeline (history, project reports).

![17-timeline](gallery/out/17-timeline.png)

### [18-gantt.fitd](gallery/18-gantt.fitd)

Gantt chart (project plans, grant proposals).

![18-gantt](gallery/out/18-gantt.png)

### [19-mind-map.fitd](gallery/19-mind-map.fitd)

Mind map / concept map (education). Radial is the default layout.

![19-mind-map](gallery/out/19-mind-map.png)

### [20-swot.fitd](gallery/20-swot.fitd)

2x2 matrix such as SWOT (business, strategy), with nodes placed on a grid.

![20-swot](gallery/out/20-swot.png)

### [21-bar-chart.fitd](gallery/21-bar-chart.fitd)

Unit bar chart (DocFigure: bar plot). Only small counts; real plots belong to a plotting tool.

![21-bar-chart](gallery/out/21-bar-chart.png)

### [22-commutative.fitd](gallery/22-commutative.fitd)

Commutative diagram (mathematics): a square placed on a grid.

![22-commutative](gallery/out/22-commutative.png)

### [23-fishbone.fitd](gallery/23-fishbone.fitd)

Cause-and-effect (Ishikawa) diagram (quality management), drawn as a tree into the effect.

![23-fishbone](gallery/out/23-fishbone.png)

### [24-swimlane.fitd](gallery/24-swimlane.fitd)

Swimlane activity diagram (Reggio et al.). Each group is a lane, in declaration order.

![24-swimlane](gallery/out/24-swimlane.png)

### [25-sequence.fitd](gallery/25-sequence.fitd)

UML sequence diagram (Reggio et al., Nickerson: second UML type). Lanes keep message order.

![25-sequence](gallery/out/25-sequence.png)

### [26-venn.fitd](gallery/26-venn.fitd)

Venn diagram of two sets (concept overlap, comparisons).

![26-venn](gallery/out/26-venn.png)

### [27-venn-three.fitd](gallery/27-venn-three.fitd)

Venn diagram of three sets, every overlap labelled (skills, feature comparisons).

![27-venn-three](gallery/out/27-venn-three.png)

### [28-algorithm.fitd](gallery/28-algorithm.fitd)

Algorithm block (DocFigure and ACL-Fig: "algorithm" class).

![28-algorithm](gallery/out/28-algorithm.png)

### [29-image.fitd](gallery/29-image.fitd)

Mixed figure: a plot inside a pipeline (Viziometrics, DocFigure, ACL-Fig mix plots and diagrams).

![29-image](gallery/out/29-image.png)
